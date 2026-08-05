use std::collections::HashMap;
use std::sync::Arc;

use tauri::{ipc::Channel, State};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::state::{AppState, TauriChannelEmitter};
use mockproxyrs_core::domain::{
    Method, MockRule, MockRuleDTO, MockService, MockServiceDetail, ResponseEvent, ServiceStatus,
};
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

/// 添加规则
#[tauri::command]
pub async fn add_rule(params: MockRuleDTO, state: State<'_, AppState>) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    let is_regex = params.is_regex.unwrap_or(false);
    let rule = MockRule::new(
        id.clone(),
        params.service_id.expect("service_id expect!"),
        params.url_pattern.expect("url_pattern expect!"),
        is_regex,
        parse_method(&params.method.unwrap_or_default()),
        params.enabled.unwrap_or(false),
        params.forward_and_record.unwrap_or(false),
        params.mock_response.unwrap_or_default(),
        params.script,
        params.delay_ms,
        params.use_script.unwrap_or(false),
    );

    state
        .repository
        .save_rule(&rule)
        .await
        .map_err(|e| e.to_string())?;

    // 更新运行中服务的规则
    let services = state.services.read().await;
    if let Some(running) = services.get(&rule.service_id) {
        running.add_rule(rule).await;
    }

    Ok(id)
}

/// 更新规则
#[tauri::command]
pub async fn update_rule(
    id: String,
    service_id: String,
    url_pattern: String,
    is_regex: bool,
    method: String,
    enabled: bool,
    forward_and_record: bool,
    mock_response: String,
    script: Option<String>,
    delay_ms: Option<u64>,
    use_script: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let rule = MockRule::new(
        id.clone(),
        service_id.clone(),
        url_pattern,
        is_regex,
        parse_method(&method),
        enabled,
        forward_and_record,
        mock_response,
        script.filter(|s| !s.is_empty()),
        delay_ms,
        use_script,
    );

    state
        .repository
        .save_rule(&rule)
        .await
        .map_err(|e| e.to_string())?;

    // 更新运行中服务的规则
    let services = state.services.read().await;
    if let Some(running) = services.get(&service_id) {
        running.update_rule(rule).await;
    }

    Ok(())
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

/// 删除规则
#[tauri::command]
pub async fn delete_rule(id: String, state: State<'_, AppState>) -> Result<(), String> {
    // 先获取规则信息
    let rule = state
        .repository
        .get_rule(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("规则不存在")?;

    let service_id = rule.service_id.clone();

    state
        .repository
        .delete_rule(&id)
        .await
        .map_err(|e| e.to_string())?;

    // 更新运行中服务的规则
    let services = state.services.read().await;
    if let Some(running) = services.get(&service_id) {
        running.delete_rule(&id).await;
    }

    Ok(())
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

/// 校验 JS 脚本语法（仅诊断，不影响保存）
#[tauri::command]
pub async fn validate_script(script: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .script_engine
        .validate(&script)
        .map_err(|e| e.to_string())
}
