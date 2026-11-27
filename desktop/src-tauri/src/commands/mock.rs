use std::collections::HashMap;
use std::sync::Arc;

use tauri::{ipc::Channel, State};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::state::{AppState, TauriChannelEmitter};
use mockproxyrs_core::domain::{
    Method, MockRule, MockRuleDTO, MockService, MockServiceDetail, ResponseEvent, ResponseKind,
    ServiceStatus, UpdateRuleParams,
};
use mockproxyrs_core::event::RuleChangeEvent;
use mockproxyrs_core::proxy::{ProxyServer, RuleMatcher};
use mockproxyrs_core::MockRepository;

/// 获取所有服务
#[tauri::command]
pub async fn list_services(state: State<'_, AppState>) -> Result<Vec<MockService>, String> {
    state
        .repository
        .list_services()
        .await
        .map_err(|e| e.to_string())
}

/// 获取服务状态
#[tauri::command]
pub async fn list_service_status(state: State<'_, AppState>) -> Result<Vec<ServiceStatus>, String> {
    let services = state.services.read().await;
    let status_list: Vec<ServiceStatus> = services
        .values()
        .map(|s| ServiceStatus::running(s.config.id.clone()))
        .collect();
    Ok(status_list)
}

/// 获取单个服务
#[tauri::command]
pub async fn get_service(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<MockServiceDetail>, String> {
    let service = state
        .repository
        .get_service(&id)
        .await
        .map_err(|e| e.to_string())?;

    match service {
        Some(s) => {
            let running = state.is_service_running(&id).await;
            Ok(Some(MockServiceDetail::from_service(s, running)))
        }
        None => Ok(None),
    }
}

/// 添加服务
#[tauri::command]
pub async fn add_service(
    name: String,
    listen_addr: String,
    target_url: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    let service = MockService::new(id.clone(), name, listen_addr, target_url);

    state
        .repository
        .save_service(&service)
        .await
        .map_err(|e| e.to_string())?;

    Ok(id)
}

/// 更新服务
#[tauri::command]
pub async fn update_service(
    id: String,
    name: String,
    listen_addr: String,
    target_url: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // 检查服务是否运行中
    if state.is_service_running(&id).await {
        return Err("服务运行中，请先停止服务".to_string());
    }

    let service = MockService::new(id, name, listen_addr, target_url);

    state
        .repository
        .save_service(&service)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// 删除服务
#[tauri::command]
pub async fn delete_service(id: String, state: State<'_, AppState>) -> Result<(), String> {
    // 检查服务是否运行中
    if state.is_service_running(&id).await {
        return Err("服务运行中，请先停止服务".to_string());
    }

    // 删除规则
    state
        .repository
        .delete_rules_by_service(&id)
        .await
        .map_err(|e| e.to_string())?;

    // 删除服务
    state
        .repository
        .delete_service(&id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// 启动服务
#[tauri::command]
pub async fn start_service(id: String, state: State<'_, AppState>) -> Result<(), String> {
    // 检查是否已运行
    if state.is_service_running(&id).await {
        return Err("服务已在运行中".to_string());
    }

    // 获取服务配置
    let service = state
        .repository
        .get_service(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("服务不存在")?;

    // 获取规则
    let rules = state
        .repository
        .list_rules(&id)
        .await
        .map_err(|e| e.to_string())?;

    let rules_map: HashMap<String, MockRule> =
        rules.into_iter().map(|r| (r.id.clone(), r)).collect();
    let rules = Arc::new(RwLock::new(rules_map));

    // 创建关闭通道
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // 获取事件发射器
    let emitter: Arc<dyn mockproxyrs_core::event::EventEmitter> = {
        let channel = state.event_channel.clone();
        Arc::new(TauriChannelEmitter::new(channel))
    };
    // 读取规则
    let rules_guard = rules.read().await;
    // 构建匹配器并匹配规则
    let matcher = Arc::new(RwLock::new(RuleMatcher::build(&rules_guard)));
    drop(rules_guard);

    // 创建代理服务器
    let server = Arc::new(ProxyServer::new(
        service.clone(),
        rules.clone(),
        emitter,
        matcher.clone(),
        state.script_engine.clone(),
        shutdown_tx,
    ));
    let running_service = server.clone();
    // 启动服务器
    tokio::spawn(async move {
        if let Err(e) = server.start(shutdown_rx).await {
            log::error!("Proxy server error: {}", e);
        }
    });

    let mut services = state.services.write().await;
    services.insert(id, running_service);

    Ok(())
}

/// 停止服务
#[tauri::command]
pub async fn stop_service(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut services = state.services.write().await;

    let running = services.remove(&id).ok_or("服务未运行")?;

    // 发送关闭信号
    running
        .shutdown_tx
        .send(true)
        .map_err(|e| format!("发送关闭信号失败: {}", e))?;

    Ok(())
}

/// 获取服务的规则列表
#[tauri::command]
pub async fn list_rules(
    service_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<MockRule>, String> {
    state
        .repository
        .list_rules(&service_id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取单条规则（含响应集与激活项）
#[tauri::command]
pub async fn get_rule(id: String, state: State<'_, AppState>) -> Result<MockRule, String> {
    state
        .repository
        .get_rule(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("规则不存在: {}", id))
}

/// 添加规则
#[tauri::command]
pub async fn add_rule(params: MockRuleDTO, state: State<'_, AppState>) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    let is_regex = params.is_regex.unwrap_or(false);
    let rule = MockRule::new(
        id.clone(),
        params.service_id.ok_or("service_id 不能为空")?,
        params.url_pattern.ok_or("url_pattern 不能为空")?,
        is_regex,
        parse_method(&params.method.unwrap_or_default()),
        params.enabled.unwrap_or(false),
        params.forward_and_record.unwrap_or(false),
        params.mock_response.unwrap_or_default(),
        params.delay_ms,
        params.use_script.unwrap_or(false),
    );

    state
        .rule_store()
        .save_rule(rule)
        .await
        .map_err(|e| e.to_string())?;

    Ok(id)
}

/// 更新规则（只改传入的字段，其余保持不变）
///
/// 不触碰响应变体与激活项：它们由 add_variant / select_variant 等命令单独维护，
/// 这样本端用过期快照保存规则也不会覆盖另一端刚改的变体。
#[tauri::command]
pub async fn update_rule(
    params: UpdateRuleParams,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .rule_store()
        .update_rule(&params.id, |rule| {
            if let Some(v) = params.url_pattern {
                rule.url_pattern = v;
            }
            if let Some(v) = params.is_regex {
                rule.is_regex = v;
            }
            if let Some(v) = params.method {
                rule.method = parse_method(&v);
            }
            if let Some(v) = params.enabled {
                rule.enabled = v;
            }
            if let Some(v) = params.forward_and_record {
                rule.forward_and_record = v;
            }
            if let Some(v) = params.mock_response {
                rule.mock_response = v;
            }
            if let Some(v) = params.delay_ms {
                rule.delay_ms = Some(v);
            }
            if let Some(v) = params.use_script {
                rule.use_script = v;
            }
        })
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 解析方法字符串为 Method 枚举
fn parse_method(s: &str) -> Method {
    match s.to_uppercase().as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        _ => Method::All,
    }
}

// ==================== 响应变体 ====================

/// 新增响应变体（追加到响应集末尾），kind 为 static / script，返回更新后的完整规则
#[tauri::command]
pub async fn add_variant(
    rule_id: String,
    name: String,
    mock_response: String,
    kind: Option<String>,
    activate: Option<bool>,
    state: State<'_, AppState>,
) -> Result<MockRule, String> {
    let kind = parse_kind(kind.as_deref())?;
    state
        .rule_store()
        .add_variant(
            &rule_id,
            &name,
            kind,
            &mock_response,
            activate.unwrap_or(true),
        )
        .await
        .map_err(|e| e.to_string())
}

/// 重命名响应变体，返回更新后的完整规则
#[tauri::command]
pub async fn rename_variant(
    rule_id: String,
    variant_id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<MockRule, String> {
    state
        .rule_store()
        .rename_variant(&rule_id, &variant_id, &name)
        .await
        .map_err(|e| e.to_string())
}

/// 更新响应变体的类型与内容（单一内容槽：切类型的同时内容被替换），返回更新后的完整规则
#[tauri::command]
pub async fn update_variant(
    rule_id: String,
    variant_id: String,
    kind: String,
    mock_response: String,
    state: State<'_, AppState>,
) -> Result<MockRule, String> {
    let kind = parse_kind(Some(&kind))?;
    state
        .rule_store()
        .update_variant(&rule_id, &variant_id, kind, &mock_response)
        .await
        .map_err(|e| e.to_string())
}

/// 解析响应类型字符串（static / script）
fn parse_kind(s: Option<&str>) -> Result<ResponseKind, String> {
    match s {
        None | Some("static") => Ok(ResponseKind::Static),
        Some("script") => Ok(ResponseKind::Script),
        Some(other) => Err(format!("无效的响应类型: {}（可选 static / script）", other)),
    }
}

/// 切换规则当前激活的响应变体；variant_id 为 null 表示取消激活（回退到规则本身的 mockResponse）
#[tauri::command]
pub async fn select_variant(
    rule_id: String,
    variant_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<MockRule, String> {
    state
        .rule_store()
        .set_selected_variant(&rule_id, variant_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// 删除响应变体（删激活项时回退到规则本身的 mockResponse），返回更新后的完整规则
#[tauri::command]
pub async fn delete_variant(
    rule_id: String,
    variant_id: String,
    state: State<'_, AppState>,
) -> Result<MockRule, String> {
    state
        .rule_store()
        .delete_variant(&rule_id, &variant_id)
        .await
        .map_err(|e| e.to_string())
}

/// 删除规则
#[tauri::command]
pub async fn delete_rule(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .rule_store()
        .delete_rule(&id)
        .await
        .map_err(|e| e.to_string())
}

/// 删除服务的所有规则
#[tauri::command]
pub async fn delete_rules_by_service(
    service_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .repository
        .delete_rules_by_service(&service_id)
        .await
        .map_err(|e| e.to_string())?;

    // 更新运行中服务的规则
    let services = state.services.read().await;
    if let Some(running) = services.get(&service_id) {
        running.delete_all_rules().await;
    }

    Ok(())
}

/// 创建事件通道
#[tauri::command]
pub async fn create_channel(
    on_event: Channel<ResponseEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut channel = state.event_channel.write().await;
    *channel = Some(on_event);
    // 查询已启动的服务，将事件通道注入

    Ok(())
}

/// 销毁事件通道
#[tauri::command]
pub async fn destroy_channel(state: State<'_, AppState>) -> Result<(), String> {
    let mut channel = state.event_channel.write().await;
    *channel = None;
    // 查询已启动的服务，将事件通道注销

    Ok(())
}

/// 订阅规则变更事件（应用启动后常驻订阅，不随网络监控录制开关）
#[tauri::command]
pub async fn subscribe_rule_changes(
    on_change: Channel<RuleChangeEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    *state.rule_change_channel.write().await = Some(on_change);
    Ok(())
}

/// 校验 JS 脚本语法（仅诊断，不影响保存）
#[tauri::command]
pub async fn validate_script(script: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .script_engine
        .validate(&script)
        .map_err(|e| e.to_string())
}
