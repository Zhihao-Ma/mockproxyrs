// Copyright 2024 mazao
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! MCP Streamable HTTP 传输
//!
//! 在本机回环地址上监听 `POST /mcp`，把请求体作为一条 JSON-RPC 消息交给协议层处理，
//! 以 `application/json` 回包；通知返回 202 无正文。无会话状态，不支持服务端主动推送（GET）。

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Bytes;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use log::{error, info};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::error::{MockproxyrsError, Result};

use super::protocol::handle_message;
use super::tools::McpTools;

/// 默认监听端口
pub const DEFAULT_MCP_PORT: u16 = 27120;

/// 单条请求体大小上限（规则可能携带较大的 mock 响应体）
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

/// MCP 服务器运行状态（供前端展示）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub running: bool,
    pub port: Option<u16>,
    /// AI 客户端配置用的完整地址，如 "http://127.0.0.1:27120/mcp"
    pub url: Option<String>,
}

impl McpStatus {
    pub fn stopped() -> Self {
        Self {
            running: false,
            port: None,
            url: None,
        }
    }

    pub fn running(addr: SocketAddr) -> Self {
        Self {
            running: true,
            port: Some(addr.port()),
            url: Some(endpoint_url(addr.port())),
        }
    }
}

/// 给定端口对应的 MCP 端点地址
pub fn endpoint_url(port: u16) -> String {
    format!("http://127.0.0.1:{}/mcp", port)
}

/// 运行中的 MCP HTTP 服务器句柄
pub struct McpHttpServer {
    /// 实际监听地址
    pub addr: SocketAddr,
    /// 关闭信号发送器
    pub shutdown_tx: watch::Sender<bool>,
}

impl McpHttpServer {
    /// 绑定地址并在后台开始服务
    ///
    /// 端口占用等绑定错误在此同步返回；返回的句柄用于查询地址和发送关闭信号。
    pub async fn start(addr: SocketAddr, tools: Arc<McpTools>) -> Result<Self> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(MockproxyrsError::Io)?;
        let addr = listener.local_addr().map_err(MockproxyrsError::Io)?;
        info!("MCP server listening on {}", addr);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        tokio::spawn(serve(listener, tools, shutdown_rx));

        Ok(Self { addr, shutdown_tx })
    }
}

/// 主循环：接受连接直到收到关闭信号
async fn serve(
    listener: TcpListener,
    tools: Arc<McpTools>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _)) => {
                        let io = TokioIo::new(stream);
                        let tools = tools.clone();
                        let mut shutdown_rx_for_conn = shutdown_rx.clone();
                        tokio::spawn(async move {
                            let service = service_fn(move |req| {
                                let tools = tools.clone();
                                async move { handle(req, tools).await }
                            });
                            tokio::select! {
                                result = http1::Builder::new().serve_connection(io, service) => {
                                    if let Err(err) = result {
                                        error!("Error serving MCP connection: {:?}", err);
                                    }
                                }
                                _ = shutdown_rx_for_conn.changed() => {}
                            }
                        });
                    }
                    Err(e) => error!("Error accepting MCP connection: {}", e),
                }
            }
            _ = shutdown_rx.changed() => {
                info!("MCP server on {} shutting down", listener.local_addr().map(|a| a.to_string()).unwrap_or_default());
                break;
            }
        }
    }
}

/// 处理单个 HTTP 请求
async fn handle(
    req: Request<hyper::body::Incoming>,
    tools: Arc<McpTools>,
) -> std::result::Result<Response<Full<Bytes>>, Infallible> {
    let (parts, body) = req.into_parts();
    let path = parts.uri.path();

    if path != "/mcp" && path != "/" {
        return Ok(text_response(StatusCode::NOT_FOUND, "Not found"));
    }

    match parts.method {
        Method::POST => {
            // 超出大小上限时 collect 返回错误，这里统一回 413
            let bytes = match Limited::new(body, MAX_BODY_BYTES).collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    return Ok(text_response(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Request body too large",
                    ));
                }
            };
            let raw = String::from_utf8_lossy(&bytes);

            match handle_message(&raw, &tools).await {
                // 通知：已受理，无响应正文
                None => Ok(Response::builder()
                    .status(StatusCode::ACCEPTED)
                    .body(Full::new(Bytes::new()))
                    .expect("static response")),
                Some(json) => Ok(json_response(json)),
            }
        }
        // 无会话状态，客户端结束会话直接确认
        Method::DELETE => Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::new()))
            .expect("static response")),
        // GET 用于服务端主动推送（SSE），本实现不支持
        _ => Ok(text_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "Method not allowed",
        )),
    }
}

fn json_response(body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Full::new(Bytes::from(body)))
        .expect("static response")
}

fn text_response(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )
        .body(Full::new(Bytes::from_static(body.as_bytes())))
        .expect("static response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MockService;
    use crate::proxy::RuleStore;
    use crate::repository::{MockRepository, SqliteRepository};
    use hyper_util::client::legacy::Client;
    use hyper_util::client::legacy::connect::HttpConnector;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    type Http = Client<HttpConnector, Full<Bytes>>;

    /// 内存仓库 + 空服务表组成的工具上下文
    fn make_tools(repo: Arc<SqliteRepository>) -> Arc<McpTools> {
        let services = Arc::new(RwLock::new(HashMap::new()));
        Arc::new(McpTools::new(Arc::new(RuleStore::new(
            repo as Arc<dyn MockRepository>,
            services,
        ))))
    }

    async fn post(
        client: &Http,
        uri: &str,
        body: &'static [u8],
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method(Method::POST)
            .header(CONTENT_TYPE, "application/json")
            .uri(uri)
            .body(Full::new(Bytes::from_static(body)))
            .unwrap();
        let resp = client.request(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value)
    }

    /// 端到端：启动真实服务器，用 hyper 客户端完成 initialize → 通知 → tools/call → 关闭
    #[tokio::test]
    async fn test_http_roundtrip_and_shutdown() {
        let repo = Arc::new(SqliteRepository::in_memory().unwrap());
        repo.save_service(&MockService::new(
            "svc".into(),
            "Test".into(),
            "127.0.0.1:0".into(),
            "http://example.com".into(),
        ))
        .await
        .unwrap();

        // 端口 0：由系统分配空闲端口，避免测试间冲突
        let server = McpHttpServer::start("127.0.0.1:0".parse().unwrap(), make_tools(repo.clone()))
            .await
            .unwrap();
        let endpoint = format!("http://{}/mcp", server.addr);
        let client: Http = Client::builder(hyper_util::rt::TokioExecutor::new()).build_http();

        let (status, v) = post(
            &client,
            &endpoint,
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["result"]["serverInfo"]["name"], "mockproxyrs");

        // 通知 → 202 无正文
        let (status, _) = post(
            &client,
            &endpoint,
            br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);

        // 工具调用走真实仓库
        let (status, v) = post(
            &client,
            &endpoint,
            br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_services","arguments":{}}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            v["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("svc")
        );

        // GET 用于服务端推送，不支持
        let resp = client
            .request(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("http://{}/", server.addr))
                    .body(Full::new(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);

        // 未知路径
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("http://{}/nope", server.addr))
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            client.request(req).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );

        // 关闭后端口可立即重新绑定，证明监听已退出
        let port = server.addr.port();
        server.shutdown_tx.send(true).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let restarted = McpHttpServer::start(
            format!("127.0.0.1:{}", port).parse().unwrap(),
            make_tools(repo),
        )
        .await;
        assert!(restarted.is_ok(), "旧服务器应已释放端口");
    }

    #[test]
    fn test_mcp_status() {
        let stopped = McpStatus::stopped();
        assert!(!stopped.running);
        assert_eq!(stopped.url, None);

        let running = McpStatus::running("127.0.0.1:27120".parse().unwrap());
        assert!(running.running);
        assert_eq!(running.port, Some(27120));
        assert_eq!(running.url.as_deref(), Some("http://127.0.0.1:27120/mcp"));
        assert_eq!(endpoint_url(1234), "http://127.0.0.1:1234/mcp");
    }
}
