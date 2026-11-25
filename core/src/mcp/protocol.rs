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

//! MCP 协议层
//!
//! 实现 JSON-RPC 2.0 消息到 MCP 方法的分发：
//! `initialize` / `ping` / `tools/list` / `tools/call`，其余通知不回包。
//! 传输无关，HTTP 与 stdio 桥接都只需把一条原始 JSON 文本交给 [`handle_message`]。

use serde_json::{Value, json};

use super::tools::{McpTools, ToolResult};

/// 服务器支持的最新协议版本
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// 支持的协议版本；客户端请求的版本不在其中时回退到最新版本
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// JSON-RPC 错误码：解析失败
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC 错误码：请求不合法
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC 错误码：方法不存在
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC 错误码：参数不合法（含未知工具、工具参数缺失/类型错误）
pub const INVALID_PARAMS: i64 = -32602;

/// 服务器名称（用于 initialize 的 serverInfo）
const SERVER_NAME: &str = "mockproxyrs";

/// 提供给 AI 的使用说明（随 initialize 返回）
const INSTRUCTIONS: &str = "Mockproxyrs 桌面应用的规则管理接口。\
    先调用 list_services 获取 serviceId，再用它查询或创建规则。\
    规则变更会立即同步到运行中的代理服务，无需重启；服务未运行时变更只落库，下次启动生效。\
    规则的 mockResponse 是静态响应；开启 useScript 后由 script 动态生成；\
    响应集（responseVariants）可存多条命名响应，selectedVariantId 指定当前生效的一条。";

/// 处理一条 JSON-RPC 消息，返回需要写回客户端的 JSON 文本
///
/// 通知（无 `id`）不产生响应，返回 `None`。解析失败、未知方法等协议错误以 JSON-RPC error 回包。
pub async fn handle_message(raw: &str, tools: &McpTools) -> Option<String> {
    let value: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                PARSE_ERROR,
                &format!("Parse error: {}", e),
            ));
        }
    };

    let Some(req) = value.as_object() else {
        return Some(error_response(
            Value::Null,
            INVALID_REQUEST,
            "Request must be a JSON-RPC object",
        ));
    };

    // 通知（包括 notifications/initialized、notifications/cancelled）无需响应
    let id = req.get("id")?;
    let method = req
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => initialize(&params),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": McpTools::definitions() })),
        "tools/call" => tools
            .call_from_params(&params)
            .await
            .map(ToolResult::into_result),
        _ => Err((METHOD_NOT_FOUND, format!("Method not found: {}", method))),
    };

    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
        Err((code, message)) => error_response(id.clone(), code, &message),
    })
}

/// 处理 initialize：协商协议版本并声明能力
fn initialize(params: &Value) -> std::result::Result<Value, (i64, String)> {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(LATEST_PROTOCOL_VERSION);
    let version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
        requested
    } else {
        LATEST_PROTOCOL_VERSION
    };

    Ok(json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    }))
}

/// 构造 JSON-RPC error 响应文本
fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::RuleStore;
    use crate::repository::{MockRepository, SqliteRepository};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    async fn tools() -> McpTools {
        let repo = Arc::new(SqliteRepository::in_memory().unwrap());
        let services = Arc::new(RwLock::new(HashMap::new()));
        McpTools::new(Arc::new(RuleStore::new(
            repo as Arc<dyn MockRepository>,
            services,
        )))
    }

    fn id_of(raw: &str) -> Value {
        let v: Value = serde_json::from_str(raw).unwrap();
        v["id"].clone()
    }

    #[tokio::test]
    async fn test_parse_error() {
        let tools = tools().await;
        let resp = handle_message("not json", &tools).await.unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], PARSE_ERROR);
        assert_eq!(v["id"], Value::Null);
    }

    #[tokio::test]
    async fn test_initialize_negotiates_version() {
        let tools = tools().await;
        let resp = handle_message(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            &tools,
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(v["result"]["serverInfo"]["name"], "mockproxyrs");
        assert!(v["result"]["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn test_initialize_falls_back_to_latest() {
        let tools = tools().await;
        let resp = handle_message(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
            &tools,
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn test_notification_has_no_response() {
        let tools = tools().await;
        assert!(
            handle_message(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &tools
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn test_ping_and_unknown_method() {
        let tools = tools().await;
        let resp = handle_message(r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#, &tools)
            .await
            .unwrap();
        assert_eq!(id_of(&resp), json!("p"));
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"], json!({}));

        let resp = handle_message(r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#, &tools)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_tools_list() {
        let tools = tools().await;
        let resp = handle_message(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#, &tools)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        let names: Vec<&str> = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"list_services"));
        assert!(names.contains(&"create_rule"));
        assert!(names.contains(&"select_response_variant"));
    }

    #[tokio::test]
    async fn test_tools_call_unknown_tool_is_protocol_error() {
        let tools = tools().await;
        let resp = handle_message(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
            &tools,
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_tools_call_execution_error_is_in_result() {
        let tools = tools().await;
        let resp = handle_message(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_rule","arguments":{"rule_id":"missing"}}}"#,
            &tools,
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true);
        assert!(v["error"].is_null());
    }
}
