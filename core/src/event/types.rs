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

use crate::domain::ResponseEvent;
use crate::error::Result;
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

    #[test]
    fn test_noop_emitter_default() {
        let emitter = NoopEmitter::default();
        let _ = emitter;
    }

    #[test]
    fn test_noop_emitter_clone() {
        let emitter = NoopEmitter;
        let cloned = emitter.clone();
        let _ = cloned;
    }
}
