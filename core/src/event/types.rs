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

//! 事件发射器类型定义

use crate::domain::{MockRule, ResponseEvent};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// 事件发射器 trait
///
/// 定义事件推送接口，不同平台可提供不同实现：
///
/// - Tauri 应用使用 `TauriChannelEmitter`
/// - 测试环境使用 `NoopEmitter`
pub trait EventEmitter: Send + Sync {
    /// 发送事件
    ///
    /// 返回异步 Future，支持非阻塞事件推送。
    fn emit(&self, event: ResponseEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

/// 空实现
///
/// 用于测试或不需要事件推送的场景，调用 `emit` 时不执行任何操作。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopEmitter;

impl EventEmitter for NoopEmitter {
    fn emit(&self, _event: ResponseEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        // 什么都不做
        Box::pin(async { Ok(()) })
    }
}

/// 规则变更事件（从后端推送）
///
/// 规则被任一入口（桌面端、MCP、将来的 web 端）新增或修改后推送完整规则，
/// 被删除后只推送 ID。前端据此局部刷新，使多个页面同时编辑时能看到彼此的改动。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RuleChangeEvent {
    /// 新增或更新：携带仓库中的最新完整规则
    Updated { rule: MockRule },
    /// 删除
    #[serde(rename_all = "camelCase")]
    Deleted { rule_id: String, service_id: String },
}

/// 规则变更事件发射器 trait
///
/// 与 [`EventEmitter`]（请求/响应事件，随网络监控录制开关）分开：规则变更通道由应用
/// 启动时常驻订阅，不依赖录制状态。
pub trait RuleChangeEmitter: Send + Sync {
    /// 发送规则变更事件
    fn emit_rule_change(
        &self,
        event: RuleChangeEvent,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

/// 规则变更事件的空实现
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopRuleChangeEmitter;

impl RuleChangeEmitter for NoopRuleChangeEmitter {
    fn emit_rule_change(
        &self,
        _event: RuleChangeEvent,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_emitter() {
        let emitter = NoopEmitter;
        let event = ResponseEvent::new(
            "test-service".to_string(),
            "Test Service".to_string(),
            "http://example.com/api".to_string(),
            Some("http://example.com/api".to_string()),
            "ALL".to_string(),
            false,
            None,
            false,
            false,
            "response".to_string(),
            None,
        );
        // NoopEmitter should always return Ok
        let result = emitter.emit(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_rule_change_event_serializes_with_type_tag() {
        let event = RuleChangeEvent::Deleted {
            rule_id: "r1".to_string(),
            service_id: "s1".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "deleted");
        assert_eq!(json["ruleId"], "r1");
        assert_eq!(json["serviceId"], "s1");

        let emitter = NoopRuleChangeEmitter;
        assert!(emitter.emit_rule_change(event).await.is_ok());
    }
}
