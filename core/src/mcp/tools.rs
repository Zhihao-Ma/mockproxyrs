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

//! MCP 工具定义与执行
//!
//! 每个工具对应一段 JSON Schema 参数描述和一个执行分支。两类错误分开上报：
//! 参数缺失/类型错误/未知工具属于协议错误（JSON-RPC -32602），表示调用方式不对；
//! 执行过程中的业务失败（规则不存在等）放在结果里以 `isError: true` 返回，表示操作没成功。

use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::protocol::INVALID_PARAMS;
use crate::domain::{Method, MockResponseVariant, MockRule};
use crate::proxy::RuleStore;

/// 工具参数（JSON 对象）
type Args = Map<String, Value>;
/// 参数错误：消息会以协议错误 -32602 返回
type ArgsResult<T> = std::result::Result<T, String>;

/// 工具执行结果（MCP CallToolResult 的内容部分）
pub struct ToolResult {
    text: String,
    is_error: bool,
}

impl ToolResult {
    /// 成功结果：负载以格式化 JSON 文本返回
    fn ok(payload: &impl Serialize) -> Self {
        Self {
            text: serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string()),
            is_error: false,
        }
    }

    /// 执行失败结果
    fn error(message: impl Into<String>) -> Self {
        Self {
            text: message.into(),
            is_error: true,
        }
    }

    /// 转为 MCP `tools/call` 的 result 对象
    pub fn into_result(self) -> Value {
        let mut result = json!({
            "content": [{ "type": "text", "text": self.text }],
        });
        if self.is_error {
            result["isError"] = json!(true);
        }
        result
    }
}

/// MCP 工具执行上下文：持有规则与服务的统一存取入口
pub struct McpTools {
    store: Arc<RuleStore>,
}

impl McpTools {
    pub fn new(store: Arc<RuleStore>) -> Self {
        Self { store }
    }

    /// 工具定义列表（用于 `tools/list`）
    pub fn definitions() -> Vec<Value> {
        let s = |description: &str| json!({ "type": "string", "description": description });
        let b = |description: &str| json!({ "type": "boolean", "description": description });
        let method = json!({
            "type": "string",
            "enum": ["ALL", "GET", "POST", "PUT", "DELETE"],
            "description": "HTTP 方法，ALL 匹配所有方法",
        });
        let service_id = s("服务 ID，可通过 list_services 获取");
        let rule_id = s("规则 ID");
        let variant_id = s("响应变体 ID，可通过 list_response_variants 获取");

        vec![
            tool(
                "list_services",
                "列出所有 Mock 服务及其运行状态。创建或查询规则前先调用此工具获取 serviceId。",
                json!({}),
                &[],
            ),
            tool(
                "list_rules",
                "列出指定服务下的全部 Mock 规则，含响应集与当前激活的响应变体。",
                json!({ "service_id": service_id }),
                &["service_id"],
            ),
            tool(
                "get_rule",
                "获取单条 Mock 规则的完整配置。",
                json!({ "rule_id": rule_id }),
                &["rule_id"],
            ),
            tool(
                "create_rule",
                "在指定服务下创建 Mock 规则并返回新规则。url_pattern 为请求路径（如 /api/users），\
                 is_regex=true 时为正则表达式；未传的字段取默认值，其中 enabled 默认 true。",
                json!({
                    "service_id": service_id,
                    "url_pattern": s("URL 匹配模式"),
                    "method": method,
                    "is_regex": b("是否按正则匹配，默认 false"),
                    "mock_response": s("静态 mock 响应内容"),
                    "enabled": b("是否启用 mock，默认 true"),
                    "forward_and_record": b("是否转发并记录真实响应，默认 false"),
                    "delay_ms": json!({ "type": "integer", "minimum": 0, "description": "延迟响应毫秒数" }),
                    "use_script": b("是否由 JS 脚本动态生成响应，默认 false"),
                    "script": s("JS 脚本内容（函数体，return 返回响应）"),
                }),
                &["service_id", "url_pattern"],
            ),
            tool(
                "update_rule",
                "更新 Mock 规则，只传需要修改的字段，其余保持不变。\
                 script / delay_ms / selected_variant_id 传 null 表示清空。返回更新后的规则。",
                json!({
                    "rule_id": rule_id,
                    "url_pattern": s("URL 匹配模式"),
                    "method": method,
                    "is_regex": b("是否按正则匹配"),
                    "mock_response": s("静态 mock 响应内容"),
                    "enabled": b("是否启用 mock"),
                    "forward_and_record": b("是否转发并记录真实响应"),
                    "delay_ms": json!({ "type": ["integer", "null"], "minimum": 0, "description": "延迟响应毫秒数" }),
                    "use_script": b("是否由 JS 脚本动态生成响应"),
                    "script": json!({ "type": ["string", "null"], "description": "JS 脚本内容" }),
                    "selected_variant_id": json!({ "type": ["string", "null"], "description": "当前生效的响应变体 ID，null 表示使用规则本身的 mockResponse" }),
                }),
                &["rule_id"],
            ),
            tool(
                "set_rule_enabled",
                "启用或禁用 Mock 规则，返回更新后的规则。",
                json!({ "rule_id": rule_id, "enabled": b("true 启用，false 禁用") }),
                &["rule_id", "enabled"],
            ),
            tool(
                "delete_rule",
                "删除 Mock 规则。",
                json!({ "rule_id": rule_id }),
                &["rule_id"],
            ),
            tool(
                "list_response_variants",
                "列出规则的响应集（多条命名响应）及当前激活项。",
                json!({ "rule_id": rule_id }),
                &["rule_id"],
            ),
            tool(
                "add_response_variant",
                "向规则响应集新增一条命名响应，默认同时激活，返回更新后的规则。",
                json!({
                    "rule_id": rule_id,
                    "name": s("变体名称，如 \"成功响应\""),
                    "mock_response": s("响应体内容"),
                    "activate": b("是否立即激活该变体，默认 true"),
                }),
                &["rule_id", "name", "mock_response"],
            ),
            tool(
                "select_response_variant",
                "切换规则当前生效的响应变体，返回更新后的规则。",
                json!({ "rule_id": rule_id, "variant_id": variant_id }),
                &["rule_id", "variant_id"],
            ),
            tool(
                "delete_response_variant",
                "删除一条响应变体。若删除的是激活项，则激活剩余的第一条（没有剩余则回退到规则本身的 mockResponse）。\
                 返回更新后的规则。",
                json!({ "rule_id": rule_id, "variant_id": variant_id }),
                &["rule_id", "variant_id"],
            ),
        ]
    }

    /// 处理 `tools/call` 的 params：解析工具名与参数并执行
    pub async fn call_from_params(
        &self,
        params: &Value,
    ) -> std::result::Result<ToolResult, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| (INVALID_PARAMS, "Missing tool name".to_string()))?;
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Args::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return Err((INVALID_PARAMS, "arguments must be an object".to_string()));
            }
        };
        self.call(name, &args)
            .await
            .map_err(|message| (INVALID_PARAMS, message))
    }

    /// 保存规则并以保存后的规则作为工具结果
    ///
    /// create / update / 变更响应集都走这里，让 AI 拿到最新完整状态。
    async fn save(&self, rule: MockRule) -> ArgsResult<ToolResult> {
        match self.store.save_rule(rule.clone()).await {
            Ok(()) => Ok(ToolResult::ok(&rule)),
            Err(e) => Ok(ToolResult::error(e.to_string())),
        }
    }

    /// 按名称执行工具；参数错误以 Err 返回，业务失败以 `isError` 结果返回
    pub async fn call(&self, name: &str, args: &Args) -> ArgsResult<ToolResult> {
        match name {
            "list_services" => {
                let services = match self.store.list_services().await {
                    Ok(list) => list,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };
                let mut out = Vec::new();
                for s in services {
                    let running = self.store.is_service_running(&s.id).await;
                    out.push(json!({
                        "id": s.id,
                        "name": s.name,
                        "listenAddr": s.listen_addr,
                        "targetUrl": s.target_url,
                        "running": running,
                    }));
                }
                Ok(ToolResult::ok(&out))
            }
            "list_rules" => {
                let service_id = required_str(args, "service_id")?;
                finish(self.store.list_rules(&service_id).await)
            }
            "get_rule" => {
                let rule_id = required_str(args, "rule_id")?;
                finish(self.store.get_rule(&rule_id).await)
            }
            "create_rule" => {
                let service_id = required_str(args, "service_id")?;
                let url_pattern = required_str(args, "url_pattern")?;
                let mut rule = MockRule::new(
                    Uuid::new_v4().to_string(),
                    service_id,
                    url_pattern,
                    false,
                    Method::All,
                    true,
                    false,
                    String::new(),
                    None,
                    None,
                    false,
                );
                apply_rule_patch(&mut rule, args)?;
                self.save(rule).await
            }
            "update_rule" => {
                let rule_id = required_str(args, "rule_id")?;
                let mut rule = match self.store.get_rule(&rule_id).await {
                    Ok(rule) => rule,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };
                apply_rule_patch(&mut rule, args)?;
                if let Some(id) = &rule.selected_variant_id
                    && !rule.response_variants.iter().any(|v| &v.id == id)
                {
                    return Err(format!("selected_variant_id 不存在: {}", id));
                }
                self.save(rule).await
            }
            "set_rule_enabled" => {
                let rule_id = required_str(args, "rule_id")?;
                let enabled = required_bool(args, "enabled")?;
                let mut rule = match self.store.get_rule(&rule_id).await {
                    Ok(rule) => rule,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };
                rule.enabled = enabled;
                self.save(rule).await
            }
            "delete_rule" => {
                let rule_id = required_str(args, "rule_id")?;
                match self.store.delete_rule(&rule_id).await {
                    Ok(()) => Ok(ToolResult::ok(&json!({ "ok": true, "id": rule_id }))),
                    Err(e) => Ok(ToolResult::error(e.to_string())),
                }
            }
            "list_response_variants" => {
                let rule_id = required_str(args, "rule_id")?;
                let rule = match self.store.get_rule(&rule_id).await {
                    Ok(rule) => rule,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };
                let variants: Vec<Value> = rule
                    .response_variants
                    .iter()
                    .map(|v| {
                        json!({
                            "id": v.id,
                            "name": v.name,
                            "active": rule.selected_variant_id.as_deref() == Some(v.id.as_str()),
                        })
                    })
                    .collect();
                Ok(ToolResult::ok(&json!({
                    "selectedVariantId": rule.selected_variant_id,
                    "variants": variants,
                })))
            }
            "add_response_variant" => {
                let rule_id = required_str(args, "rule_id")?;
                let name = required_str(args, "name")?;
                let mock_response = required_str(args, "mock_response")?;
                let activate = optional_bool(args, "activate")?.unwrap_or(true);
                let mut rule = match self.store.get_rule(&rule_id).await {
                    Ok(rule) => rule,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };
                let variant = MockResponseVariant {
                    id: Uuid::new_v4().to_string(),
                    rule_id: rule.id.clone(),
                    name,
                    mock_response,
                };
                if activate {
                    rule.selected_variant_id = Some(variant.id.clone());
                }
                rule.response_variants.push(variant);
                self.save(rule).await
            }
            "select_response_variant" => {
                let rule_id = required_str(args, "rule_id")?;
                let variant_id = required_str(args, "variant_id")?;
                let mut rule = match self.store.get_rule(&rule_id).await {
                    Ok(rule) => rule,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };
                if !rule.response_variants.iter().any(|v| v.id == variant_id) {
                    return Ok(ToolResult::error(format!("响应变体不存在: {}", variant_id)));
                }
                rule.selected_variant_id = Some(variant_id);
                self.save(rule).await
            }
            "delete_response_variant" => {
                let rule_id = required_str(args, "rule_id")?;
                let variant_id = required_str(args, "variant_id")?;
                let mut rule = match self.store.get_rule(&rule_id).await {
                    Ok(rule) => rule,
                    Err(e) => return Ok(ToolResult::error(e.to_string())),
                };
                let Some(pos) = rule
                    .response_variants
                    .iter()
                    .position(|v| v.id == variant_id)
                else {
                    return Ok(ToolResult::error(format!("响应变体不存在: {}", variant_id)));
                };
                rule.response_variants.remove(pos);
                if rule.selected_variant_id.as_deref() == Some(variant_id.as_str()) {
                    rule.selected_variant_id = rule.response_variants.first().map(|v| v.id.clone());
                }
                self.save(rule).await
            }
            other => Err(format!("Unknown tool: {}", other)),
        }
    }
}

/// 构造一条工具定义
fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        },
    })
}

/// 把仓库/服务操作的结果转为工具结果：成功返回负载，失败返回 isError
fn finish<T: Serialize>(result: crate::error::Result<T>) -> ArgsResult<ToolResult> {
    Ok(match result {
        Ok(value) => ToolResult::ok(&value),
        Err(e) => ToolResult::error(e.to_string()),
    })
}

/// 把调用方提供的可选字段应用到规则上；未提供的字段保持不变
///
/// 可空字段（script / delay_ms / selected_variant_id）传 null 表示清空。
fn apply_rule_patch(rule: &mut MockRule, args: &Args) -> ArgsResult<()> {
    if let Some(v) = args.get("url_pattern") {
        rule.url_pattern = value_as_str(v, "url_pattern")?;
    }
    if let Some(v) = args.get("method") {
        let raw = value_as_str(v, "method")?;
        rule.method = serde_json::from_value(Value::String(raw.to_uppercase()))
            .map_err(|_| format!("Invalid method: {} (expected ALL/GET/POST/PUT/DELETE)", raw))?;
    }
    if let Some(v) = args.get("is_regex") {
        rule.is_regex = value_as_bool(v, "is_regex")?;
    }
    if let Some(v) = args.get("enabled") {
        rule.enabled = value_as_bool(v, "enabled")?;
    }
    if let Some(v) = args.get("forward_and_record") {
        rule.forward_and_record = value_as_bool(v, "forward_and_record")?;
    }
    if let Some(v) = args.get("mock_response") {
        rule.mock_response = value_as_str(v, "mock_response")?;
    }
    if let Some(v) = args.get("use_script") {
        rule.use_script = value_as_bool(v, "use_script")?;
    }
    if let Some(v) = args.get("script") {
        rule.script = value_as_opt_str(v, "script")?.filter(|s| !s.is_empty());
    }
    if let Some(v) = args.get("delay_ms") {
        rule.delay_ms = value_as_opt_u64(v, "delay_ms")?;
    }
    if let Some(v) = args.get("selected_variant_id") {
        rule.selected_variant_id = value_as_opt_str(v, "selected_variant_id")?;
    }
    Ok(())
}

// ---- 参数解析 ----

fn required_str(args: &Args, key: &str) -> ArgsResult<String> {
    match args.get(key) {
        Some(v) => value_as_str(v, key),
        None => Err(format!("Missing required argument: {}", key)),
    }
}

fn required_bool(args: &Args, key: &str) -> ArgsResult<bool> {
    match args.get(key) {
        Some(v) => value_as_bool(v, key),
        None => Err(format!("Missing required argument: {}", key)),
    }
}

fn optional_bool(args: &Args, key: &str) -> ArgsResult<Option<bool>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => value_as_bool(v, key).map(Some),
    }
}

fn value_as_str(v: &Value, key: &str) -> ArgsResult<String> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("Argument must be a string: {}", key))
}

fn value_as_opt_str(v: &Value, key: &str) -> ArgsResult<Option<String>> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(format!("Argument must be a string or null: {}", key)),
    }
}

fn value_as_bool(v: &Value, key: &str) -> ArgsResult<bool> {
    v.as_bool()
        .ok_or_else(|| format!("Argument must be a boolean: {}", key))
}

fn value_as_opt_u64(v: &Value, key: &str) -> ArgsResult<Option<u64>> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) if n.is_u64() => Ok(n.as_u64()),
        _ => Err(format!(
            "Argument must be a non-negative integer or null: {}",
            key
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MockService;
    use crate::repository::{MockRepository, SqliteRepository};
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    async fn setup() -> (McpTools, String) {
        let repo = Arc::new(SqliteRepository::in_memory().unwrap());
        let service = MockService::new(
            "svc-1".into(),
            "Test".into(),
            "127.0.0.1:8080".into(),
            "http://example.com".into(),
        );
        repo.save_service(&service).await.unwrap();
        let services = Arc::new(RwLock::new(HashMap::new()));
        let tools = McpTools::new(Arc::new(RuleStore::new(
            repo as Arc<dyn MockRepository>,
            services,
        )));
        (tools, service.id)
    }

    fn args(pairs: &[(&str, Value)]) -> Args {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn test_definitions_have_schema() {
        for t in McpTools::definitions() {
            assert!(t["name"].is_string());
            assert!(t["description"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[tokio::test]
    async fn test_create_update_and_get_rule() {
        let (tools, svc) = setup().await;

        let r = tools
            .call(
                "create_rule",
                &args(&[
                    ("service_id", json!(svc)),
                    ("url_pattern", json!("/api/users")),
                ]),
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert!(rule.enabled, "enabled 默认 true");
        assert_eq!(rule.method, Method::All);

        let r = tools
            .call(
                "update_rule",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("method", json!("post")),
                    ("delay_ms", json!(500)),
                    ("mock_response", json!("{}")),
                ]),
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        let updated: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(updated.method, Method::Post, "method 大小写不敏感");
        assert_eq!(updated.delay_ms, Some(500));
        assert_eq!(updated.url_pattern, "/api/users", "未传字段保持不变");

        let r = tools
            .call(
                "update_rule",
                &args(&[("rule_id", json!(rule.id)), ("delay_ms", Value::Null)]),
            )
            .await
            .unwrap();
        let updated: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(updated.delay_ms, None, "null 清空字段");

        let r = tools
            .call("get_rule", &args(&[("rule_id", json!(rule.id))]))
            .await
            .unwrap();
        let fetched: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(fetched, updated);
    }

    #[tokio::test]
    async fn test_variant_lifecycle() {
        let (tools, svc) = setup().await;
        let r = tools
            .call(
                "create_rule",
                &args(&[("service_id", json!(svc)), ("url_pattern", json!("/a"))]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();

        let r = tools
            .call(
                "add_response_variant",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("name", json!("成功")),
                    ("mock_response", json!("ok")),
                ]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(rule.response_variants.len(), 1);
        let first = rule.response_variants[0].id.clone();
        assert_eq!(
            rule.selected_variant_id.as_deref(),
            Some(first.as_str()),
            "默认激活"
        );

        let r = tools
            .call(
                "add_response_variant",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("name", json!("失败")),
                    ("mock_response", json!("err")),
                    ("activate", json!(false)),
                ]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        let second = rule.response_variants[1].id.clone();
        assert_eq!(
            rule.selected_variant_id.as_deref(),
            Some(first.as_str()),
            "activate=false 不切换"
        );

        let r = tools
            .call(
                "select_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!(second))]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(rule.selected_variant_id.as_deref(), Some(second.as_str()));

        // 删除激活项 → 激活剩余第一条
        let r = tools
            .call(
                "delete_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!(second))]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(rule.selected_variant_id.as_deref(), Some(first.as_str()));

        // 删除最后一个 → 回退到 None
        let r = tools
            .call(
                "delete_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!(first))]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert!(rule.response_variants.is_empty());
        assert_eq!(rule.selected_variant_id, None);

        // 删除不存在的变体 → isError 结果而非协议错误
        let r = tools
            .call(
                "delete_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!("nope"))]),
            )
            .await
            .unwrap();
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn test_set_enabled_delete_and_list() {
        let (tools, svc) = setup().await;
        let r = tools
            .call(
                "create_rule",
                &args(&[("service_id", json!(svc)), ("url_pattern", json!("/a"))]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();

        let r = tools
            .call(
                "set_rule_enabled",
                &args(&[("rule_id", json!(rule.id)), ("enabled", json!(false))]),
            )
            .await
            .unwrap();
        assert!(!r.is_error);

        let r = tools
            .call("list_rules", &args(&[("service_id", json!(svc))]))
            .await
            .unwrap();
        let list: Vec<MockRule> = serde_json::from_str(&r.text).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].enabled);

        let r = tools.call("list_services", &Args::new()).await.unwrap();
        let v: Value = serde_json::from_str(&r.text).unwrap();
        assert_eq!(v[0]["id"], svc);
        assert_eq!(v[0]["running"], false);

        let r = tools
            .call("delete_rule", &args(&[("rule_id", json!(rule.id))]))
            .await
            .unwrap();
        assert!(!r.is_error);
        let r = tools
            .call("get_rule", &args(&[("rule_id", json!(rule.id))]))
            .await
            .unwrap();
        assert!(r.is_error, "已删除的规则返回 isError");
    }

    #[tokio::test]
    async fn test_argument_errors_are_protocol_errors() {
        let (tools, _svc) = setup().await;
        assert!(
            tools
                .call("create_rule", &args(&[("url_pattern", json!("/a"))]))
                .await
                .is_err()
        );
        assert!(
            tools
                .call(
                    "create_rule",
                    &args(&[("service_id", json!("s")), ("url_pattern", json!(3))])
                )
                .await
                .is_err()
        );
        assert!(
            tools
                .call(
                    "create_rule",
                    &args(&[
                        ("service_id", json!("s")),
                        ("url_pattern", json!("/a")),
                        ("method", json!("PATCH"))
                    ])
                )
                .await
                .is_err()
        );
        assert!(tools.call("unknown", &Args::new()).await.is_err());
    }
}
