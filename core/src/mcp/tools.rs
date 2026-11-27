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
use crate::domain::{Method, MockRule, ResponseKind};
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
            "description": "HTTP 方法，ALL 匹配所有方法；同一 urlPattern 下精确方法优先于 ALL",
        });
        let service_id = s("服务 ID，可通过 list_services 获取");
        let rule_id = s("规则 ID");
        let variant_id =
            s("响应变体 ID（不是名称），可通过 list_rules 或 list_response_variants 获取");
        let url_pattern = s(
            "URL 匹配模式。isRegex=false 时与请求 path 完全相等（不是前缀，/api/users 不命中 /api/users/123），也可写含 query 的完整 url；isRegex=true 时为正则，部分匹配、不锚定首尾。完整优先级规则见 instructions 的「URL 匹配」",
        );
        let is_regex = b("是否按正则解释 urlPattern，默认 false");
        let kind_schema = json!({
            "type": "string",
            "enum": ["static", "script"],
            "description": "响应类型：static 内容原样返回；script 内容是 JS 函数体，通过 request 读取请求、return 返回响应，完整写法见 instructions。缺省 static",
        });
        let content_desc = "响应内容，原样返回：Content-Type 固定 application/json;charset=UTF-8，不做 JSON 校验；kind=script 时是 JS 函数体";
        let variant_name = s("变体名称，仅作展示标签，脚本与匹配都引用不到；引用变体请用 id");

        vec![
            tool(
                "list_services",
                "列出所有 Mock 服务及其运行状态。创建或查询规则前先调用此工具获取 serviceId。",
                json!({}),
                &[],
            ),
            tool(
                "list_rules",
                "列出指定服务下的全部 Mock 规则，已含响应集与当前激活项，通常无需再调 list_response_variants。",
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
                    "url_pattern": url_pattern,
                    "method": method,
                    "is_regex": is_regex,
                    "mock_response": s(&format!("默认响应内容（规则未选中变体时生效）。{content_desc}")),
                    "enabled": b("是否启用 mock，默认 true。注意未启用的规则仍参与匹配并遮住后面的规则，只是命中后按透传处理"),
                    "forward_and_record": b("是否转发并记录真实响应，默认 false"),
                    "delay_ms": json!({ "type": ["integer", "null"], "minimum": 0, "description": "延迟响应毫秒数" }),
                    "use_script": b("默认响应是否为 JS 脚本（函数体，写法见 instructions），默认 false（静态）"),
                }),
                &["service_id", "url_pattern"],
            ),
            tool(
                "update_rule",
                "更新 Mock 规则，只传需要修改的字段，其余保持不变。delay_ms 传 null 表示清空。\
                 不改变响应变体与激活项，请用 select_response_variant 等工具。返回更新后的规则。",
                json!({
                    "rule_id": rule_id,
                    "url_pattern": url_pattern,
                    "method": method,
                    "is_regex": is_regex,
                    "mock_response": s(&format!("默认响应内容（规则本身的响应，选中变体时以变体为准）。{content_desc}")),
                    "enabled": b("是否启用 mock。注意未启用的规则仍参与匹配并遮住后面的规则，只是命中后按透传处理"),
                    "forward_and_record": b("是否转发并记录真实响应"),
                    "delay_ms": json!({ "type": ["integer", "null"], "minimum": 0, "description": "延迟响应毫秒数" }),
                    "use_script": b("默认响应是否为 JS 脚本（函数体，写法见 instructions）"),
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
                "列出规则的响应集（多条命名响应）及当前激活项。list_rules 的返回已包含同样信息，此工具用于只关心单个规则的场景。",
                json!({ "rule_id": rule_id }),
                &["rule_id"],
            ),
            tool(
                "add_response_variant",
                "向规则响应集新增一条命名响应，默认同时激活，返回更新后的规则。",
                json!({
                    "rule_id": rule_id,
                    "name": variant_name,
                    "content": s(content_desc),
                    "kind": kind_schema,
                    "activate": b("是否立即激活该变体，默认 true"),
                }),
                &["rule_id", "name", "content"],
            ),
            tool(
                "rename_response_variant",
                "重命名一条响应变体（仅改展示标签），返回更新后的规则。",
                json!({ "rule_id": rule_id, "variant_id": variant_id, "name": variant_name }),
                &["rule_id", "variant_id", "name"],
            ),
            tool(
                "update_response_variant",
                "更新一条响应变体的类型与内容。每条变体只有一个内容槽：传入 content 即整体替换；传 kind 则同时切换类型。\
                 不传 kind 时保持原类型。返回更新后的规则。",
                json!({
                    "rule_id": rule_id,
                    "variant_id": variant_id,
                    "content": s(content_desc),
                    "kind": kind_schema,
                }),
                &["rule_id", "variant_id", "content"],
            ),
            tool(
                "select_response_variant",
                "切换规则当前生效的响应变体；variant_id 传 null 表示取消激活，回退到规则本身的 mockResponse。返回更新后的规则。",
                json!({
                    "rule_id": rule_id,
                    "variant_id": json!({ "type": ["string", "null"], "description": "响应变体 ID，可通过 list_response_variants 获取；null 取消激活" }),
                }),
                &["rule_id"],
            ),
            tool(
                "delete_response_variant",
                "删除一条响应变体。若删除的是激活项，则回退到规则本身的 mockResponse。返回更新后的规则。",
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
                    false,
                );
                apply_rule_patch(&mut rule, args)?;
                finish(self.store.save_rule(rule).await)
            }
            "update_rule" => {
                let rule_id = required_str(args, "rule_id")?;
                // 先解析参数：类型错误以协议错误返回，不进入闭包
                let patch = RulePatch::from_args(args)?;
                finish(
                    self.store
                        .update_rule(&rule_id, |rule| patch.apply(rule))
                        .await,
                )
            }
            "set_rule_enabled" => {
                let rule_id = required_str(args, "rule_id")?;
                let enabled = required_bool(args, "enabled")?;
                finish(
                    self.store
                        .update_rule(&rule_id, |rule| rule.enabled = enabled)
                        .await,
                )
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
                            "kind": v.kind,
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
                let content = required_str(args, "content")?;
                let kind = optional_kind(args)?.unwrap_or_default();
                let activate = optional_bool(args, "activate")?.unwrap_or(true);
                finish(
                    self.store
                        .add_variant(&rule_id, &name, kind, &content, activate)
                        .await,
                )
            }
            "rename_response_variant" => {
                let rule_id = required_str(args, "rule_id")?;
                let variant_id = required_str(args, "variant_id")?;
                let name = required_str(args, "name")?;
                finish(
                    self.store
                        .rename_variant(&rule_id, &variant_id, &name)
                        .await,
                )
            }
            "update_response_variant" => {
                let rule_id = required_str(args, "rule_id")?;
                let variant_id = required_str(args, "variant_id")?;
                let content = required_str(args, "content")?;
                // 未传 kind 时保持原类型
                let kind = match optional_kind(args)? {
                    Some(kind) => kind,
                    None => {
                        let rule = match self.store.get_rule(&rule_id).await {
                            Ok(rule) => rule,
                            Err(e) => return Ok(ToolResult::error(e.to_string())),
                        };
                        match rule.response_variants.iter().find(|v| v.id == variant_id) {
                            Some(v) => v.kind,
                            None => {
                                return Ok(ToolResult::error(format!(
                                    "响应变体不存在: {}",
                                    variant_id
                                )));
                            }
                        }
                    }
                };
                finish(
                    self.store
                        .update_variant(&rule_id, &variant_id, kind, &content)
                        .await,
                )
            }
            "select_response_variant" => {
                let rule_id = required_str(args, "rule_id")?;
                // 变体参数可选：缺省或 null 表示取消激活
                let variant_id = match args.get("variant_id") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(value_as_str(v, "variant_id")?),
                };
                finish(
                    self.store
                        .set_selected_variant(&rule_id, variant_id.as_deref())
                        .await,
                )
            }
            "delete_response_variant" => {
                let rule_id = required_str(args, "rule_id")?;
                let variant_id = required_str(args, "variant_id")?;
                finish(self.store.delete_variant(&rule_id, &variant_id).await)
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
///
/// 规则写入类工具都以仓库中的最新完整规则作为负载（含其它端刚写入的变体）。
fn finish<T: Serialize>(result: crate::error::Result<T>) -> ArgsResult<ToolResult> {
    Ok(match result {
        Ok(value) => ToolResult::ok(&value),
        Err(e) => ToolResult::error(e.to_string()),
    })
}

/// 已解析的规则字段补丁：只携带调用方传了的字段，`apply` 不会失败
///
/// 可空字段（script / delay_ms）传 null 表示清空。响应变体与激活项不在此处，
/// 由 select_response_variant 等工具单独维护。
#[derive(Default)]
struct RulePatch {
    url_pattern: Option<String>,
    method: Option<String>,
    is_regex: Option<bool>,
    enabled: Option<bool>,
    forward_and_record: Option<bool>,
    mock_response: Option<String>,
    use_script: Option<bool>,
    delay_ms: Option<Option<u64>>,
}

impl RulePatch {
    /// 解析工具参数；类型错误以协议错误返回
    fn from_args(args: &Args) -> ArgsResult<Self> {
        let mut patch = Self::default();
        if let Some(v) = args.get("url_pattern") {
            patch.url_pattern = Some(value_as_str(v, "url_pattern")?);
        }
        if let Some(v) = args.get("method") {
            let raw = value_as_str(v, "method")?;
            // 提前校验，非法方法以协议错误返回
            serde_json::from_value::<Method>(Value::String(raw.to_uppercase())).map_err(|_| {
                format!("Invalid method: {} (expected ALL/GET/POST/PUT/DELETE)", raw)
            })?;
            patch.method = Some(raw);
        }
        if let Some(v) = args.get("is_regex") {
            patch.is_regex = Some(value_as_bool(v, "is_regex")?);
        }
        if let Some(v) = args.get("enabled") {
            patch.enabled = Some(value_as_bool(v, "enabled")?);
        }
        if let Some(v) = args.get("forward_and_record") {
            patch.forward_and_record = Some(value_as_bool(v, "forward_and_record")?);
        }
        if let Some(v) = args.get("mock_response") {
            patch.mock_response = Some(value_as_str(v, "mock_response")?);
        }
        if let Some(v) = args.get("use_script") {
            patch.use_script = Some(value_as_bool(v, "use_script")?);
        }
        if let Some(v) = args.get("delay_ms") {
            patch.delay_ms = Some(value_as_opt_u64(v, "delay_ms")?);
        }
        Ok(patch)
    }

    /// 把携带的字段应用到规则上；未携带的字段保持不变
    fn apply(&self, rule: &mut MockRule) {
        if let Some(v) = &self.url_pattern {
            rule.url_pattern = v.clone();
        }
        if let Some(raw) = &self.method {
            // from_args 已校验过，这里不可能失败
            rule.method =
                serde_json::from_value(Value::String(raw.to_uppercase())).unwrap_or(Method::All);
        }
        if let Some(v) = self.is_regex {
            rule.is_regex = v;
        }
        if let Some(v) = self.enabled {
            rule.enabled = v;
        }
        if let Some(v) = self.forward_and_record {
            rule.forward_and_record = v;
        }
        if let Some(v) = &self.mock_response {
            rule.mock_response = v.clone();
        }
        if let Some(v) = self.use_script {
            rule.use_script = v;
        }
        if let Some(v) = self.delay_ms {
            rule.delay_ms = v;
        }
    }
}

/// 把调用方提供的可选字段应用到规则上；未提供的字段保持不变
///
/// 可空字段 delay_ms 传 null 表示清空。默认响应由 `use_script`（类型）+ `mock_response`（内容）描述。
fn apply_rule_patch(rule: &mut MockRule, args: &Args) -> ArgsResult<()> {
    RulePatch::from_args(args)?.apply(rule);
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

/// 解析可选的响应类型参数（`kind`）；非法值以协议错误返回
fn optional_kind(args: &Args) -> ArgsResult<Option<ResponseKind>> {
    match args.get("kind") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let raw = value_as_str(v, "kind")?;
            serde_json::from_value::<ResponseKind>(Value::String(raw.clone()))
                .map(Some)
                .map_err(|_| format!("Invalid kind: {} (expected static/script)", raw))
        }
    }
}

fn value_as_str(v: &Value, key: &str) -> ArgsResult<String> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("Argument must be a string: {}", key))
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
    use crate::event::NoopRuleChangeEmitter;
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
            Arc::new(NoopRuleChangeEmitter),
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
                    ("content", json!("ok")),
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
                    ("content", json!("err")),
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

        // rename / update_response_variant 是行级操作，不影响激活项
        let r = tools
            .call(
                "rename_response_variant",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("variant_id", json!(first)),
                    ("name", json!("成功 v2")),
                ]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(rule.response_variants[0].name, "成功 v2");
        // update_response_variant：传 kind=script 切换类型并整体替换内容
        let r = tools
            .call(
                "update_response_variant",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("variant_id", json!(first)),
                    ("kind", json!("script")),
                    ("content", json!("return { status: 200, body: 'ok2' };")),
                ]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(rule.response_variants[0].kind, ResponseKind::Script);
        assert_eq!(
            rule.response_variants[0].content,
            "return { status: 200, body: 'ok2' };"
        );
        assert_eq!(
            rule.selected_variant_id.as_deref(),
            Some(second.as_str()),
            "内容操作不切换激活"
        );
        // 不传 kind 时保持原类型
        let r = tools
            .call(
                "update_response_variant",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("variant_id", json!(first)),
                    ("content", json!("return { status: 201, body: 'created' };")),
                ]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(
            rule.response_variants[0].kind,
            ResponseKind::Script,
            "kind 缺省保持不变"
        );
        // 非法 kind → 协议错误
        assert!(
            tools
                .call(
                    "update_response_variant",
                    &args(&[
                        ("rule_id", json!(rule.id)),
                        ("variant_id", json!(first)),
                        ("kind", json!("yaml")),
                        ("content", json!("x")),
                    ]),
                )
                .await
                .is_err()
        );

        // variant_id 传 null → 取消激活，回退到规则本身的 mockResponse
        let r = tools
            .call(
                "select_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", Value::Null)]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(rule.selected_variant_id, None);
        assert_eq!(
            rule.effective_response(),
            (ResponseKind::Static, "".into()),
            "回退到规则本身（创建时为空串）"
        );

        // 重新激活 second，再删除它 → 回退到规则本身（不再自动选剩余第一条）
        tools
            .call(
                "select_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!(second))]),
            )
            .await
            .unwrap();
        let r = tools
            .call(
                "delete_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!(second))]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(rule.response_variants.len(), 1);
        assert_eq!(rule.selected_variant_id, None, "删除激活项后回退到规则本身");

        // 删除最后一个变体
        let r = tools
            .call(
                "delete_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!(first))]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        assert!(rule.response_variants.is_empty());

        // 删除不存在的变体 → isError 结果而非协议错误
        let r = tools
            .call(
                "delete_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!("nope"))]),
            )
            .await
            .unwrap();
        assert!(r.is_error);

        // 选中不存在的变体 → isError
        let r = tools
            .call(
                "select_response_variant",
                &args(&[("rule_id", json!(rule.id)), ("variant_id", json!("nope"))]),
            )
            .await
            .unwrap();
        assert!(r.is_error);
    }

    /// 分表后的核心保证：加变体后用 update_rule 改规则字段，变体保留
    #[tokio::test]
    async fn test_update_rule_does_not_touch_variants() {
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
                    ("content", json!("ok")),
                ]),
            )
            .await
            .unwrap();
        let rule: MockRule = serde_json::from_str(&r.text).unwrap();
        let variant_id = rule.response_variants[0].id.clone();

        // 改 URL、方法、开关、默认响应（切为脚本）：全是规则自身的字段
        let r = tools
            .call(
                "update_rule",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("url_pattern", json!("/b")),
                    ("method", json!("POST")),
                    ("enabled", json!(false)),
                    ("use_script", json!(true)),
                    ("mock_response", json!("return {};")),
                ]),
            )
            .await
            .unwrap();
        assert!(!r.is_error);
        let updated: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(updated.url_pattern, "/b");
        assert_eq!(updated.method, Method::Post);
        assert!(!updated.enabled);
        assert_eq!(
            updated.default_kind(),
            ResponseKind::Script,
            "默认响应切为脚本"
        );
        assert_eq!(
            updated.response_variants.len(),
            1,
            "变体不受 update_rule 影响"
        );
        assert_eq!(updated.response_variants[0].id, variant_id);
        assert_eq!(
            updated.selected_variant_id.as_deref(),
            Some(variant_id.as_str()),
            "激活项不受影响"
        );

        // update_rule 不再接受 selected_variant_id（被忽略，不报错）
        let r = tools
            .call(
                "update_rule",
                &args(&[
                    ("rule_id", json!(rule.id)),
                    ("selected_variant_id", Value::Null),
                ]),
            )
            .await
            .unwrap();
        let updated: MockRule = serde_json::from_str(&r.text).unwrap();
        assert_eq!(
            updated.selected_variant_id.as_deref(),
            Some(variant_id.as_str())
        );
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
