//! 领域模型模块
//!
//! 定义核心业务实体，包括 Mock 服务、规则、事件等。

mod mock_config;
mod proxy_config;

pub use mock_config::{
    Method, MockRule, MockRuleDTO, MockService, MockServiceDetail, ResponseEvent, ServiceStatus,
};
pub use proxy_config::ForwardProxyConfig;
