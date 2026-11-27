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
///
/// 按「流程 → 数据模型 → 脚本 API → 约定」组织。脚本 API 与 Content-Type 约定只在这里有完整说明，
/// 工具描述里只放一句话指向此处；改动脚本运行时或响应约定时必须同步更新，测试会校验关键词。
const INSTRUCTIONS: &str = r#"Mockproxyrs 桌面应用的规则管理接口。本说明覆盖全部工具的调用流程、数据模型、脚本 API 与响应约定。

## 调用流程
1. 先调用 list_services 获取 serviceId（含 running 运行状态）。
2. 用 serviceId 调 list_rules 查询规则，或 create_rule 创建规则；create/update 类工具都返回更新后的完整规则。
3. 规则变更立即同步到运行中的代理服务，无需重启；服务未运行时只落库，下次启动生效。

## 数据模型
规则（MockRule）：
- id、serviceId
- urlPattern：匹配模式，匹配语义见下文「URL 匹配」
- isRegex：urlPattern 是否按正则解释
- method：ALL / GET / POST / PUT / DELETE（大小写不敏感）
- enabled：是否启用 mock。注意：未启用的规则仍参与匹配并会遮住后面的规则，只是命中后按透传处理
- forwardAndRecord：开启后先转发到上游并记录真实响应，但仍返回 mock 响应
- delayMs：延迟响应毫秒数
- 默认响应 = useScript（类型）+ mockResponse（内容）
- responseVariants：响应集，每条 { id, ruleId, name, kind, content }
- selectedVariantId：当前生效的变体 id；null 表示使用默认响应

每条响应（默认响应或变体）都有自己的类型，且只有一个内容槽：
- static：content 原样作为响应体返回
- script：content 是 JS 函数体，按请求动态生成响应（见下文）

当前生效的响应 = selectedVariantId 指向的变体（类型 + 内容），否则是默认响应。切换类型会替换内容。

## URL 匹配（最容易写出与预期不符的规则，务必读完）
请求 URL 分为 path（? 之前）与完整 url（含 query）两部分参与匹配。按以下顺序尝试，命中即停：
1. isRegex=false：path 与 urlPattern 字符串完全相等（不是前缀匹配，/api/users 不会命中 /api/users/123）。先找 method 相同的规则，再找 method=ALL 的规则
2. isRegex=false：完整 url（含 query）与 urlPattern 完全相等，如 /api/users?id=1。先 method 相同，再 method=ALL
3. isRegex=true：对 path 做正则“部分匹配”（regex.is_match，不锚定首尾），/api/users 会命中 /api/users/123 和 /api/users-admin。要全路径匹配请自己写 ^...$。多条正则同时命中时取哪一条不确定（无优先级），请让正则互不重叠
方法过滤在每一层内部：精确方法优先于 ALL，但精确匹配层整体优先于正则层。
典型坑：
- 想匹配 /api/users/123 这样的动态路径 → 用正则 ^/api/users/[^/]+$
- 一条 method=GET 的精确规则和一条 method=ALL 的正则规则都能命中 → 精确规则赢
- 同一 urlPattern+method 的两条精确规则 → 后保存的覆盖先保存的（匹配器以 urlPattern+method 为键）
- 多条正则都能命中 → 结果不确定，请缩小正则范围或改用精确匹配

## 脚本 API（kind=script / useScript=true）
脚本是普通函数体，用 return 返回响应，同步执行。可用全局变量：
- request.method：HTTP 方法，大写字符串（如 "GET"）
- request.url：完整路径，含 query（如 "/api/users?id=1"）
- request.path：不含 query 的路径
- request.query：对象，已做 percent-decode（如 { id: "1" }）
- request.headers：对象，键已转为小写（如 request.headers["content-type"]）
- request.body：请求体原始字符串，可能为空
- console.log(...) / console.error(...)：写入应用日志文件

return 的两种形态：
1. 返回任意非 undefined 的值：对象/数组会被 JSON.stringify，字符串原样返回，状态码 200，Content-Type 为 application/json;charset=UTF-8。例：
   return { code: 0, data: request.query };
   return "plain text";
2. 返回 { status, headers?, body? } 完整控制：status 为 100~599 的数字；headers 为字符串键值对象（键会被转为小写）；body 必须是字符串，缺省为空串，对象需自己 JSON.stringify。例：
   var id = Number(request.query.id) || 0;
   return { status: id % 2 === 0 ? 200 : 404, headers: { "x-id": String(id) }, body: JSON.stringify({ even: id % 2 === 0 }) };

限制：没有 fetch / 定时器 / require / 模块系统，只做纯计算；内存与栈有上限。语法错误、运行时异常或没有 return（返回 undefined）时，代理返回 500，响应体为 {"error":"...","code":500}。

## 响应约定
- 静态响应（kind=static）原样返回，Content-Type 固定为 application/json;charset=UTF-8，Transfer-Encoding: chunked；不做 JSON 校验，内容不必是合法 JSON。
- 需要其它 Content-Type（如 text/html）或自定义状态码/响应头时，使用 kind=script 并在返回值里显式设置 headers。
- 变体的 name 仅是展示标签，脚本与规则匹配都引用不到它；在工具参数里引用变体请用 id。
- 工具签名可能随服务端版本变化（如字段改名）；升级后请重新连接并重新读取 tools/list，不要沿用记忆中的旧签名。"#;

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
        // 版本号前置：工具签名随版本变化，AI 在上下文里就能看到当前版本而不必依赖客户端展示 serverInfo
        "instructions": format!("服务端版本 {}。\n{}", env!("CARGO_PKG_VERSION"), INSTRUCTIONS),
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
    use crate::event::NoopRuleChangeEmitter;
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
            Arc::new(NoopRuleChangeEmitter),
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

    /// instructions 是 AI 唯一的脚本 API 文档：必须覆盖关键字面量，且不能残留旧模型的字段说法
    #[tokio::test]
    async fn test_instructions_document_script_api_and_current_model() {
        let tools = tools().await;
        let resp = handle_message(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &tools,
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        let instructions = v["result"]["instructions"].as_str().unwrap();

        // 版本号前置，AI 在上下文里就能看到
        assert!(instructions.starts_with(&format!("服务端版本 {}。", env!("CARGO_PKG_VERSION"))));

        // 流程
        for kw in ["list_services", "serviceId", "create_rule", "list_rules"] {
            assert!(
                instructions.contains(kw),
                "instructions 缺少流程关键词: {kw}"
            );
        }
        // 脚本 API：request 字面量字段、console、return 两种形态、示例
        for kw in [
            "request.method",
            "request.url",
            "request.path",
            "request.query",
            "request.headers",
            "request.body",
            "console.log",
            "JSON.stringify",
            "status",
            "headers",
            "body",
            "return {",
        ] {
            assert!(
                instructions.contains(kw),
                "instructions 缺少脚本 API 关键词: {kw}"
            );
        }
        // 响应约定
        for kw in [
            "application/json;charset=UTF-8",
            "不做 JSON 校验",
            "展示标签",
            "tools/list",
        ] {
            assert!(
                instructions.contains(kw),
                "instructions 缺少约定关键词: {kw}"
            );
        }
        // URL 匹配语义：精确相等非前缀、正则部分匹配、优先级、未启用仍参与匹配
        for kw in [
            "URL 匹配",
            "不是前缀匹配",
            "regex.is_match",
            "^...$",
            "不确定",
            "未启用的规则仍参与匹配",
            "后保存的覆盖先保存的",
        ] {
            assert!(
                instructions.contains(kw),
                "instructions 缺少 URL 匹配关键词: {kw}"
            );
        }
        // 当前模型：每条响应有类型 + 单一内容槽
        for kw in [
            "kind",
            "content",
            "useScript",
            "mockResponse",
            "selectedVariantId",
        ] {
            assert!(
                instructions.contains(kw),
                "instructions 缺少数据模型关键词: {kw}"
            );
        }
        // 旧模型已删除的字段不得出现（script 列已并入 mockResponse）
        assert!(
            !instructions.contains("开启 useScript 后由 script"),
            "instructions 残留旧模型的 script 字段说法"
        );
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
