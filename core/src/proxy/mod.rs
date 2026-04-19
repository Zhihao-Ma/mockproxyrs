//! 代理服务器模块
//!
//! 提供 HTTP 代理功能，包括请求转发、规则匹配等。
//!
//! # 主要组件
//!
//! - [`ProxyServer`] - 代理服务器，监听请求并根据规则处理
//! - [`RuleMatcher`] - 规则匹配器，根据 URL 匹配 Mock 规则
//! - [`CompiledRule`] - 编译后的规则，用于高效匹配
//! - [`UpstreamClient`] - 上游客户端，转发请求到目标服务器

mod client;
mod matcher;
mod server;

pub use client::UpstreamClient;
pub use matcher::CompiledRule;
pub use matcher::RuleMatcher;
pub use server::ProxyServer;
