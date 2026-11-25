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

//! Mockproxyrs MCP stdio 桥接
//!
//! 部分 AI 客户端只支持以子进程方式拉起 MCP 服务器（stdio 传输）。本程序作为桥接：
//! 从 stdin 逐行读取 JSON-RPC 消息，转发给桌面应用内运行的 MCP HTTP 端点，
//! 再把响应写回 stdout。自身不持有任何规则逻辑，因此需要桌面应用处于运行状态且已启动 MCP 服务器。
//!
//! 用法：`mockproxyrs-mcp [--port <端口>]`，端口默认与桌面应用一致（27120）。
//! stdout 只输出 JSON-RPC 响应，日志一律走 stderr。

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header::CONTENT_TYPE;
use hyper::{Method, Request};
use mockproxyrs_core::mcp::{DEFAULT_MCP_PORT, endpoint_url};
use mockproxyrs_core::proxy::UpstreamClient;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

/// JSON-RPC 解析错误码
const PARSE_ERROR: i64 = -32700;
/// 桥接自身错误（无法连接桌面应用等），使用自定义错误码区间
const BRIDGE_ERROR: i64 = -32000;

#[tokio::main]
async fn main() {
    let port = parse_port(std::env::args().skip(1));
    let endpoint = endpoint_url(port);
    let target = format!("http://127.0.0.1:{}", port);
    eprintln!("mockproxyrs-mcp bridging stdio to {}", endpoint);

    let client = UpstreamClient::new();
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = BufWriter::new(tokio::io::stdout());

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let message: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                // 无法解析的输入按 JSON-RPC 规范回 id=null 的解析错误，便于客户端发现问题
                let resp = error_response(Value::Null, PARSE_ERROR, &format!("Parse error: {}", e));
                if write_line(&mut stdout, &resp).await.is_err() {
                    break;
                }
                continue;
            }
        };

        // 带 id 的是请求，需要回包；不带 id 的是通知，转发后丢弃响应
        let id = message.get("id").filter(|v| !v.is_null()).cloned();

        let result = forward(&client, &target, line).await;
        let Some(id) = id else {
            continue;
        };

        // 请求理论上总有正文（通知对应 202 无正文），没有则按桥接错误上报
        let response = match result {
            Ok(body) if !body.is_empty() => body,
            Ok(_) => error_response(id, BRIDGE_ERROR, "桌面应用返回了空响应"),
            Err(e) => error_response(
                id,
                BRIDGE_ERROR,
                &format!(
                    "无法连接到 Mockproxyrs 桌面应用的 MCP 服务器（{}）：{}。请确认应用已启动并已开启 MCP 服务器。",
                    endpoint, e
                ),
            ),
        };

        if write_line(&mut stdout, &response).await.is_err() {
            break;
        }
    }
}

/// 把一条 JSON-RPC 文本 POST 到桌面应用的 MCP 端点，返回响应正文
async fn forward(client: &UpstreamClient, target: &str, body: &str) -> Result<String, String> {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header(CONTENT_TYPE, "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(Full::new(Bytes::copy_from_slice(body.as_bytes())))
        .map_err(|e| e.to_string())?;

    let resp = client
        .forward(req, target)
        .await
        .map_err(|e| e.to_string())?;
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await.map_err(|e| e.to_string())?.to_bytes();
    let text = String::from_utf8_lossy(&bytes).into_owned();

    if parts.status.is_success() {
        Ok(text)
    } else {
        Err(format!("HTTP {} {}", parts.status, text))
    }
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

async fn write_line(out: &mut BufWriter<tokio::io::Stdout>, line: &str) -> std::io::Result<()> {
    out.write_all(line.as_bytes()).await?;
    out.write_all(b"\n").await?;
    out.flush().await
}

/// 解析命令行参数中的端口：`--port <n>` / `--port=<n>`，缺省为默认端口
fn parse_port(args: impl Iterator<Item = String>) -> u16 {
    let mut args = args;
    let mut port = DEFAULT_MCP_PORT;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                if let Some(v) = args.next()
                    && let Ok(p) = v.parse()
                {
                    port = p;
                }
            }
            _ if arg.starts_with("--port=") => {
                if let Ok(p) = arg["--port=".len()..].parse() {
                    port = p;
                }
            }
            "--help" | "-h" => {
                println!(
                    "mockproxyrs-mcp — Mockproxyrs MCP stdio 桥接\n\n用法: mockproxyrs-mcp [--port <端口>]\n\n将 stdin/stdout 的 JSON-RPC 转发到桌面应用内的 MCP HTTP 端点（默认 {}）。",
                    endpoint_url(DEFAULT_MCP_PORT)
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }
    port
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_parse_port() {
        let p = |args: &[&str]| parse_port(args.iter().map(|s| s.to_string()));
        assert_eq!(p(&[]), DEFAULT_MCP_PORT);
        assert_eq!(p(&["--port", "3000"]), 3000);
        assert_eq!(p(&["--port=4000"]), 4000);
        assert_eq!(
            p(&["--port", "abc"]),
            DEFAULT_MCP_PORT,
            "非法端口回退默认值"
        );
    }

    #[test]
    fn test_error_response_shape() {
        let s = error_response(json!(7), BRIDGE_ERROR, "boom");
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["error"]["code"], BRIDGE_ERROR);
        assert_eq!(v["error"]["message"], "boom");
    }

    /// 桥接转发走真实 HTTP：进程内启动 MCP 服务器，验证请求回包与通知无正文
    #[tokio::test]
    async fn test_forward_against_running_mcp_server() {
        use mockproxyrs_core::mcp::{McpHttpServer, McpTools};
        use mockproxyrs_core::proxy::RuleStore;
        use mockproxyrs_core::repository::SqliteRepository;
        use std::collections::HashMap;

        let repo = Arc::new(SqliteRepository::in_memory().unwrap());
        let services = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let tools = Arc::new(McpTools::new(Arc::new(RuleStore::new(repo, services))));
        let server = McpHttpServer::start("127.0.0.1:0".parse().unwrap(), tools)
            .await
            .unwrap();
        let target = format!("http://127.0.0.1:{}", server.addr.port());
        let client = UpstreamClient::new();

        let body = forward(
            &client,
            &target,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"], json!({}));

        // 通知对应 202 无正文
        let body = forward(
            &client,
            &target,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        )
        .await
        .unwrap();
        assert!(body.is_empty());

        // 目标未监听：返回 Err 而非 panic
        let err = forward(
            &client,
            "http://127.0.0.1:1",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        )
        .await;
        assert!(err.is_err());

        server.shutdown_tx.send(true).unwrap();
    }
}
