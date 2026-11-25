//! MCP（Model Context Protocol）服务器模块
//!
//! 让 AI 客户端通过标准 MCP 协议管理 Mock 规则。
//!
//! # 主要组件
//!
//! - [`protocol`] - JSON-RPC 消息分发（传输无关）
//! - [`tools`] - 工具定义与执行，底层复用 [`crate::proxy::RuleStore`]
//! - [`server`] - Streamable HTTP 传输
//!
//! # 接入方式
//!
//! 1. 用 `RuleStore::new(repository, running_services)` 创建统一存取入口
//! 2. `McpTools::new(store)` 包装为工具上下文
//! 3. `McpHttpServer::start(addr, tools)` 在后台开始服务，用返回的句柄发送关闭信号

mod protocol;
mod server;
mod tools;

pub use protocol::handle_message;
pub use server::{DEFAULT_MCP_PORT, McpHttpServer, McpStatus, endpoint_url};
pub use tools::{McpTools, ToolResult};
