use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use flate2::read::GzDecoder;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use log::{error, info};
use tokio::net::TcpListener;
use tokio::sync::{RwLock, watch};

use crate::domain::{MockRule, MockService, ResponseEvent};
use crate::error::{MockproxyrsError, Result};
use crate::event::EventEmitter;
use crate::mock::{RequestContext, ScriptEngine, ScriptMockResponse, execute_script};
use crate::proxy::{CompiledRule, RuleMatcher, UpstreamClient};
use regex::Regex;

type HttpBody = BoxBody<Bytes, hyper::Error>;

/// 代理服务器
pub struct ProxyServer {
    /// 服务配置
    pub config: MockService,
    /// 规则列表
    pub rules: Arc<RwLock<HashMap<String, MockRule>>>,
    /// 规则匹配器
    pub matcher: Arc<RwLock<RuleMatcher>>,
    /// 事件发射器
    event_emitter: Arc<dyn EventEmitter>,
    /// 上游客户端
    client: UpstreamClient,
    /// JS 脚本引擎
    script_engine: Arc<ScriptEngine>,
    /// 关闭信号发送器
    pub shutdown_tx: watch::Sender<bool>,
}

impl ProxyServer {
    /// 创建新的代理服务器
    pub fn new(
        config: MockService,
        rules: Arc<RwLock<HashMap<String, MockRule>>>,
        event_emitter: Arc<dyn EventEmitter>,
        matcher: Arc<RwLock<RuleMatcher>>,
        script_engine: Arc<ScriptEngine>,
        shutdown_tx: watch::Sender<bool>,
    ) -> Self {
        Self {
            config,
            rules,
            event_emitter,
            matcher,
            client: UpstreamClient::new(),
            script_engine,
            shutdown_tx,
        }
    }

    /// 启动服务器
    ///
    /// # Arguments
    /// * `shutdown_rx` - 关闭信号接收器
    ///
    /// # Returns
    /// 成功返回 Ok(())，失败返回错误
    pub async fn start(&self, mut shutdown_rx: watch::Receiver<bool>) -> Result<()> {
        let addr: SocketAddr = self.config.listen_addr.parse().map_err(|e| {
            MockproxyrsError::InvalidAddress(format!("{}: {}", self.config.listen_addr, e))
        })?;

        info!("Starting proxy server on {}", addr);

        let listener = TcpListener::bind(addr)
            .await
            .map_err(MockproxyrsError::Io)?;

        info!("Proxy server listening on {}", addr);

        // 主循环
        loop {
            tokio::select! {
                // 接受新连接
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            let io = TokioIo::new(stream);
                            let config = self.config.clone();
                            let rules = self.rules.clone();
                            let matcher = self.matcher.clone();
                            let emitter = self.event_emitter.clone();
                            let client = self.client.clone();
                            let script_engine = self.script_engine.clone();

                            let mut shutdown_rx_for_conn = shutdown_rx.clone();
                            tokio::spawn(async move {
                                let service = service_fn(move |req| {
                                    let config = config.clone();
                                    let rules = rules.clone();
                                    let matcher = matcher.clone();
                                    let emitter = emitter.clone();
                                    let client = client.clone();
                                    let script_engine = script_engine.clone();
                                    async move {
                                        handle_request(req, config, rules, matcher, emitter, client, script_engine).await
                                    }
                                });

                                tokio::select!{
                                    result = http1::Builder::new()
                                        .serve_connection(io, service)
                                            => {
                                        if let Err(err) = result {
                                            error!("Error serving connection: {:?}", err);
                                        }
                                    }
                                    _ = shutdown_rx_for_conn.changed() => {
                                        info!("Connection received shutdown signal, finishing current request and closing...");
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            error!("Error accepting connection: {}", e);
                        }
                    }
                }

                // 接收关闭信号
                _ = shutdown_rx.changed() => {
                    info!("Shutdown signal received, stopping server {}", self.config.id);
                    break;
                }
            }
        }

        Ok(())
    }

    /// 事件发射器注册、注销
    pub fn set_event_emitter(&mut self, event_emitter: Arc<dyn EventEmitter>) {
        self.event_emitter = event_emitter;
    }
}

/// 对外暴露的命令实现
impl ProxyServer {
    pub async fn add_rule(&self, rule: MockRule) {
        let mut rules = self.rules.write().await;
        let mut matcher = self.matcher.write().await;
        if rule.is_regex {
            if let Ok(regex) = Regex::new(&rule.url_pattern) {
                matcher.regex_rules.insert(
                    (rule.url_pattern.clone(), rule.method.to_string()),
                    CompiledRule {
                        rule_id: rule.id.clone(),
                        regex: Some(regex),
                    },
                );
            }
        } else {
            matcher.exact_match_map.insert(
                (rule.url_pattern.clone(), rule.method.to_string()),
                rule.id.clone(),
            );
        }
        rules.insert(rule.id.clone(), rule);
    }

    pub async fn update_rule(&self, rule: MockRule) {
        let mut rules = self.rules.write().await;
        if let Some(existing) = rules.get_mut(&rule.id) {
            let url_pattern_old = existing.url_pattern.clone();
            let is_regex_old = existing.is_regex;
            if !url_pattern_old.eq(&rule.url_pattern) || is_regex_old != rule.is_regex {
                let mut matcher = self.matcher.write().await;
                if matcher
                    .exact_match_map
                    .contains_key(&(url_pattern_old.clone(), rule.method.to_string()))
                {
                    matcher
                        .exact_match_map
                        .remove(&(url_pattern_old.clone(), rule.method.to_string()));
                }
                if is_regex_old {
                    matcher
                        .regex_rules
                        .remove(&(url_pattern_old, rule.method.to_string()));
                }
            }
        }
        let mut matcher = self.matcher.write().await;
        if rule.is_regex {
            if let Ok(regex) = Regex::new(&rule.url_pattern) {
                matcher.regex_rules.insert(
                    (rule.url_pattern.clone(), rule.method.to_string()),
                    CompiledRule {
                        rule_id: rule.id.clone(),
                        regex: Some(regex),
                    },
                );
            }
        } else {
            matcher.exact_match_map.insert(
                (rule.url_pattern.clone(), rule.method.to_string()),
                rule.id.clone(),
            );
        }

        rules.insert(rule.id.clone(), rule);
    }

    pub async fn delete_rule(&self, rule_id: &str) {
        let mut rules = self.rules.write().await;
        if let Some(rule) = rules.remove(rule_id) {
            let mut matcher = self.matcher.write().await;
            if rule.is_regex {
                matcher
                    .regex_rules
                    .remove(&(rule.url_pattern, rule.method.to_string()));
            } else {
                matcher
                    .exact_match_map
                    .remove(&(rule.url_pattern, rule.method.to_string()));
            }
        }
    }

    pub async fn delete_all_rules(&self) {
        let mut rules = self.rules.write().await;
        rules.clear();
        let mut matcher = self.matcher.write().await;
        matcher.exact_match_map.clear();
        matcher.regex_rules.clear();
    }
}

/// 处理单个请求
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    config: MockService,
    rules: Arc<RwLock<HashMap<String, MockRule>>>,
    matcher: Arc<RwLock<RuleMatcher>>,
    event_emitter: Arc<dyn EventEmitter>,
    client: UpstreamClient,
    script_engine: Arc<ScriptEngine>,
) -> std::result::Result<Response<HttpBody>, hyper::Error> {
    let url = req.uri().to_string();
    let method = req.method().clone().to_string();

    // 读取规则
    let rules_guard = rules.read().await;

    // 构建匹配器并匹配规则
    let matcher_guard = matcher.read().await;
    let matched_rule = matcher_guard.match_rule(method.as_str(), &url, &rules_guard);

    let is_mock = matched_rule.is_some_and(|r| r.enabled);
    let forward_and_record = matched_rule.is_some_and(|r| r.forward_and_record);
    let matched_rule_id = matched_rule.map(|r| r.id.clone());
    let mock_response = matched_rule.map(|r| r.mock_response.clone());
    let script = matched_rule
        .and_then(|r| r.script.clone())
        .filter(|s| !s.trim().is_empty());
    let delay_ms = matched_rule.and_then(|r| r.delay_ms).unwrap_or(0);
    let url_pattern = matched_rule.map(|r| r.url_pattern.clone());
    let is_regex = matched_rule.is_some_and(|r| r.is_regex);
    drop(matcher_guard);
    drop(rules_guard);

    info!(
        "Request: {} {}, mock: {}, forward_and_record: {}",
        url, config.name, is_mock, forward_and_record
    );

    let mut mock_body_for_event = if is_mock { mock_response.clone() } else { None };

    // 决定处理方式
    let (response, response_body) = if is_mock && script.is_some() {
        #[allow(clippy::unnecessary_unwrap)]
        let script = script.expect("script checked above");
        let (parts, body) = req.into_parts();
        let bytes = body.collect().await?.to_bytes();
        let request_body = String::from_utf8_lossy(&bytes).into_owned();
        let request_context = RequestContext::new(
            method.clone(),
            url.clone(),
            headers_to_map(&parts.headers),
            request_body,
        );

        match execute_script((*script_engine).clone(), request_context, script).await {
            Ok(script_response) => {
                mock_body_for_event = Some(script_response.body.clone());
                let response = build_script_response(script_response);

                if forward_and_record {
                    info!(
                        "Script rule has forward_and_record set; body recorded from script output"
                    );
                    (response, String::new())
                } else {
                    (response, String::new())
                }
            }
            Err(e) => {
                error!("Script execution failed: {}", e);
                (build_error_response(&e.to_string()), e.to_string())
            }
        }
    } else if !is_mock || forward_and_record {
        // 转发请求
        match client.forward(req, &config.target_url).await {
            Ok(resp) => {
                let (parts, body) = resp.into_parts();
                let bytes = body.collect().await?.to_bytes();

                // 解压响应
                let body_str = decode_response(&parts.headers, &bytes);

                // 如果是 mock 且转发，返回 mock 响应
                if is_mock {
                    let mock_resp = build_mock_response(&mock_response.clone().unwrap_or_default());
                    (mock_resp, body_str)
                } else {
                    let resp = Response::from_parts(parts, full(bytes));
                    (resp, body_str)
                }
            }
            Err(e) => {
                error!("Error forwarding request: {}", e);
                let resp = build_error_response(&e.to_string());
                (resp, e.to_string())
            }
        }
    } else {
        // 直接返回 mock 响应
        let mock_resp = build_mock_response(&mock_response.clone().unwrap_or_default());
        (mock_resp, String::new())
    };

    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    // 发送事件
    let event = ResponseEvent::new(
        config.id.clone(),
        config.name.clone(),
        url,
        url_pattern,
        method,
        is_regex,
        matched_rule_id,
        is_mock,
        forward_and_record,
        response_body,
        mock_body_for_event,
    );

    if let Err(e) = event_emitter.emit(event).await {
        error!("Error emitting event: {}", e);
    }

    Ok(response)
}

fn headers_to_map(headers: &hyper::header::HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_lowercase(), v.to_string()))
        })
        .collect()
}

/// 解压响应内容
fn decode_response(headers: &hyper::header::HeaderMap, bytes: &Bytes) -> String {
    let content_encoding = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if content_encoding.to_lowercase().contains("gzip")
        || content_encoding.to_lowercase().contains("deflate")
    {
        let mut decoder = GzDecoder::new(&bytes[..]);
        let mut decompressed = Vec::new();
        if std::io::Read::read_to_end(&mut decoder, &mut decompressed).is_ok() {
            return String::from_utf8_lossy(&decompressed).into_owned();
        }
    }

    String::from_utf8_lossy(bytes).into_owned()
}

/// 构建 mock 响应
fn build_mock_response(content: &str) -> Response<HttpBody> {
    let body = content.to_string();
    Response::builder()
        .status(200)
        .header("Content-Type", "application/json;charset=UTF-8")
        .header("Transfer-Encoding", "chunked")
        .body(full(body))
        .expect("Failed to build mock response")
}

/// 构建脚本响应
fn build_script_response(script_response: ScriptMockResponse) -> Response<HttpBody> {
    let mut builder = Response::builder().status(script_response.status);
    for (key, value) in script_response.headers {
        builder = builder.header(key, value);
    }
    builder
        .body(full(script_response.body))
        .expect("Failed to build script response")
}

/// 构建错误响应
fn build_error_response(message: &str) -> Response<HttpBody> {
    let body = serde_json::json!({
        "error": message,
        "code": 500
    })
    .to_string();

    Response::builder()
        .status(500)
        .header("Content-Type", "application/json;charset=UTF-8")
        .body(full(body))
        .expect("Failed to build error response")
}

/// 创建 Full body
fn full<T: Into<Bytes>>(chunk: T) -> HttpBody {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderMap;

    #[test]
    fn test_decode_response_plain() {
        let headers = HeaderMap::new();
        let bytes = Bytes::from("Hello, World!");

        let result = decode_response(&headers, &bytes);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_decode_response_gzip() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        let mut headers = HeaderMap::new();
        headers.insert("content-encoding", "gzip".parse().unwrap());

        // Compress the content
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"Hello, Gzip!").unwrap();
        let compressed = encoder.finish().unwrap();
        let bytes = Bytes::from(compressed);

        let result = decode_response(&headers, &bytes);
        assert_eq!(result, "Hello, Gzip!");
    }

    #[test]
    fn test_decode_response_deflate() {
        // deflate encoding is also handled by GzDecoder in this implementation
        let mut headers = HeaderMap::new();
        headers.insert("content-encoding", "deflate".parse().unwrap());

        let bytes = Bytes::from("test content");
        let result = decode_response(&headers, &bytes);
        // Since we're not actually compressing with deflate, this will return the raw bytes
        // The function tries to decode but falls back to raw bytes on error
        assert!(!result.is_empty());
    }

    #[test]
    fn test_build_mock_response() {
        let content = r#"{"code": 200, "message": "success"}"#;
        let response = build_mock_response(content);

        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/json;charset=UTF-8"
        );
    }

    #[test]
    fn test_build_script_response() {
        let response = build_script_response(ScriptMockResponse {
            status: 201,
            headers: HashMap::from([("x-test".to_string(), "ok".to_string())]),
            body: "created".to_string(),
        });

        assert_eq!(response.status(), 201);
        assert_eq!(response.headers().get("x-test").unwrap(), "ok");
    }

    #[test]
    fn test_build_error_response() {
        let message = "Internal server error";
        let response = build_error_response(message);

        assert_eq!(response.status(), 500);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/json;charset=UTF-8"
        );
    }

    #[test]
    fn test_full_body() {
        let body = full("test content");
        // Just verify it doesn't panic
        let _ = body;
    }
}
