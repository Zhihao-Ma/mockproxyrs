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

//! 应用状态管理
//!
//! 管理运行中的服务、数据仓库和事件通道。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tauri::ipc::Channel;
use tokio::sync::RwLock;

use mockproxyrs_core::domain::ResponseEvent;
use mockproxyrs_core::event::{EventEmitter, RuleChangeEmitter, RuleChangeEvent};
use mockproxyrs_core::mcp::McpHttpServer;
use mockproxyrs_core::mock::ScriptEngine;
use mockproxyrs_core::proxy::{ProxyServer, RuleStore};
use mockproxyrs_core::repository::SqliteRepository;

/// 应用全局状态
///
/// 在 Tauri 应用启动时创建，通过 `State` 注入到各个命令处理函数。
pub struct AppState {
    /// 运行中的服务映射（服务 ID -> 运行中服务）
    pub services: Arc<RwLock<HashMap<String, Arc<ProxyServer>>>>,
    /// 数据仓库（SQLite）
    pub repository: Arc<SqliteRepository>,
    /// 事件通道（用于向前端推送响应事件，随网络监控录制开关）
    pub event_channel: Arc<RwLock<Option<Channel<ResponseEvent>>>>,
    /// 规则变更通道（常驻，不随录制开关；规则被任一入口改动时向前端推送）
    pub rule_change_channel: Arc<RwLock<Option<Channel<RuleChangeEvent>>>>,
    /// JS 脚本引擎
    pub script_engine: Arc<ScriptEngine>,
    /// 运行中的 MCP 服务器句柄
    pub mcp_server: Arc<RwLock<Option<McpHttpServer>>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(repository: SqliteRepository, script_engine: ScriptEngine) -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            repository: Arc::new(repository),
            script_engine: Arc::new(script_engine),
            event_channel: Arc::new(RwLock::new(None)),
            rule_change_channel: Arc::new(RwLock::new(None)),
            mcp_server: Arc::new(RwLock::new(None)),
        }
    }

    /// 检查服务是否运行中
    pub async fn is_service_running(&self, id: &str) -> bool {
        let services = self.services.read().await;
        services.contains_key(id)
    }

    /// 规则与服务的统一存取入口
    ///
    /// 与运行中的服务表、规则变更通道共享同一份 `Arc`：规则变更会立即同步到运行中的代理服务，
    /// 并推送给前端（桌面 IPC 与 MCP 两个入口共用此路径）。
    pub fn rule_store(&self) -> RuleStore {
        RuleStore::new(
            self.repository.clone(),
            self.services.clone(),
            Arc::new(TauriRuleChangeEmitter {
                channel: self.rule_change_channel.clone(),
            }),
        )
    }
}

/// Tauri Channel 规则变更事件发射器
///
/// 前端未订阅时（channel 为 None），事件被丢弃。
#[derive(Clone)]
pub struct TauriRuleChangeEmitter {
    pub channel: Arc<RwLock<Option<Channel<RuleChangeEvent>>>>,
}

impl RuleChangeEmitter for TauriRuleChangeEmitter {
    fn emit_rule_change(
        &self,
        event: RuleChangeEvent,
    ) -> Pin<Box<dyn Future<Output = mockproxyrs_core::error::Result<()>> + Send + '_>> {
        Box::pin(async move {
            let channel_read = self.channel.read().await;
            if let Some(ch) = channel_read.as_ref() {
                ch.send(event).map_err(|e| {
                    mockproxyrs_core::error::MockproxyrsError::Channel(e.to_string())
                })?;
            }
            Ok(())
        })
    }
}

/// Tauri Channel 事件发射器
///
/// 通过 Tauri IPC Channel 将事件推送到前端。
/// 当前端未订阅时（channel 为 None），事件将被丢弃。
#[derive(Clone)]
pub struct TauriChannelEmitter {
    channel: Arc<RwLock<Option<Channel<ResponseEvent>>>>,
}

impl TauriChannelEmitter {
    /// 创建新的事件发射器
    pub fn new(channel: Arc<RwLock<Option<Channel<ResponseEvent>>>>) -> Self {
        Self { channel }
    }
}

impl EventEmitter for TauriChannelEmitter {
    fn emit(
        &self,
        event: ResponseEvent,
    ) -> Pin<Box<dyn Future<Output = mockproxyrs_core::error::Result<()>> + Send + '_>> {
        Box::pin(async move {
            let channel_read = self.channel.read().await;

            if let Some(ch) = channel_read.as_ref() {
                ch.send(event).map_err(|e| {
                    mockproxyrs_core::error::MockproxyrsError::Channel(e.to_string())
                })?;
            }
            Ok(())
        })
    }
}
