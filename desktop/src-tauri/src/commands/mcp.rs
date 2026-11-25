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

//! MCP 服务器生命周期管理
//!
//! MCP 服务器随桌面应用进程运行，通过 `RuleStore` 与 IPC 命令共享同一套规则存取逻辑，
//! AI 客户端做的变更会立即同步到运行中的代理服务。

use std::net::SocketAddr;
use std::sync::Arc;

use mockproxyrs_core::mcp::{McpHttpServer, McpStatus, McpTools, DEFAULT_MCP_PORT};
use tauri::State;

use crate::state::AppState;

/// 启动 MCP 服务器
///
/// # Arguments
/// * `port` - 监听端口，缺省使用默认端口
///
/// # Returns
/// 启动后的状态（含实际端口与端点地址）
#[tauri::command]
pub async fn start_mcp_server(
    port: Option<u16>,
    state: State<'_, AppState>,
) -> Result<McpStatus, String> {
    let mut server = state.mcp_server.write().await;
    if server.is_some() {
        return Err("MCP 服务器已在运行中".to_string());
    }

    // 仅监听本机回环地址：规则库里可能带敏感的 mock 数据，不对外网暴露
    let addr = SocketAddr::from(([127, 0, 0, 1], port.unwrap_or(DEFAULT_MCP_PORT)));
    let tools = Arc::new(McpTools::new(Arc::new(state.rule_store())));
    let handle = McpHttpServer::start(addr, tools)
        .await
        .map_err(|e| format!("启动 MCP 服务器失败: {}", e))?;
    let status = McpStatus::running(handle.addr);
    *server = Some(handle);

    Ok(status)
}

/// 停止 MCP 服务器
#[tauri::command]
pub async fn stop_mcp_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut server = state.mcp_server.write().await;
    let handle = server.take().ok_or("MCP 服务器未运行")?;

    handle
        .shutdown_tx
        .send(true)
        .map_err(|e| format!("发送关闭信号失败: {}", e))
}

/// 获取 MCP 服务器状态
#[tauri::command]
pub async fn get_mcp_status(state: State<'_, AppState>) -> Result<McpStatus, String> {
    let server = state.mcp_server.read().await;
    Ok(server
        .as_ref()
        .map(|h| McpStatus::running(h.addr))
        .unwrap_or_else(McpStatus::stopped))
}
