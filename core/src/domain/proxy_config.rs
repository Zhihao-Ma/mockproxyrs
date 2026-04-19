use serde::{Deserialize, Serialize};

/// 正向代理配置（未来功能）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForwardProxyConfig {
    /// 监听地址
    pub listen_addr: String,
    /// 上游代理地址（可选）
    pub upstream_proxy: Option<String>,
}

impl ForwardProxyConfig {
    pub fn new(listen_addr: String, upstream_proxy: Option<String>) -> Self {
        Self {
            listen_addr,
            upstream_proxy,
        }
    }
}
