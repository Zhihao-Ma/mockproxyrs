//! 事件系统模块
//!
//! 提供事件发射器接口，用于将请求/响应事件推送到前端。
//!
//! # 实现类
//!
//! - [`EventEmitter`] - 事件发射器 trait，由不同平台实现
//! - [`NoopEmitter`] - 空实现，用于测试或不需要事件推送的场景

mod types;

pub use crate::domain::ResponseEvent;
pub use types::{EventEmitter, NoopEmitter};
