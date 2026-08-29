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

//! 上游客户端
//!
//! 负责将请求转发到目标服务器，支持 HTTP 和 HTTPS。

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use crate::error::{MockproxyrsError, Result};

/// 上游请求客户端
///
/// 内部维护 HTTP 和 HTTPS 两个客户端实例。
/// HTTPS 客户端配置为接受无效证书（用于开发测试环境）。
#[derive(Debug, Clone)]
pub struct UpstreamClient {
    /// HTTP 客户端
    http_client: Client<HttpConnector, Full<Bytes>>,
    /// HTTPS 客户端
    https_client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
}

impl Default for UpstreamClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UpstreamClient {
    /// 创建新的上游客户端
    pub fn new() -> Self {
        // HTTP 客户端
        let http_client = Client::builder(TokioExecutor::new()).build_http();

        // HTTPS 客户端（忽略证书验证）
        let mut http = HttpConnector::new();
        http.enforce_http(false);

        let tls = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .expect("Failed to create TLS connector");

        let https = HttpsConnector::from((http, tokio_native_tls::TlsConnector::from(tls)));
        let https_client = Client::builder(TokioExecutor::new()).build(https);

        Self {
            http_client,
            https_client,
        }
    }

    /// 转发请求到目标地址
    ///
    /// # Arguments
    /// * `req` - 原始请求（body 为已读取的 `Full<Bytes>`）
    /// * `target_url` - 目标地址（如 "https://example.com"）
    ///
    /// # Returns
    /// 目标服务器的响应
    pub async fn forward(
        &self,
        req: Request<Full<Bytes>>,
        target_url: &str,
    ) -> Result<Response<Incoming>> {
        let (mut parts, body) = req.into_parts();

        // 构建完整 URL
        let uri = parts.uri.clone();
        let full_url = format!("{}{}", target_url, uri);

        // 更新请求
        parts.uri = full_url
            .parse()
            .map_err(|e| MockproxyrsError::Parse(format!("Invalid URL: {}", e)))?;

        // 更新 Host 头
        let host = self.extract_host(target_url);
        parts.headers.insert(
            hyper::header::HOST,
            host.parse()
                .map_err(|e| MockproxyrsError::Parse(format!("Invalid host: {}", e)))?,
        );

        // 移除 Content-Length（让 hyper 自动处理）
        parts.headers.remove(hyper::header::CONTENT_LENGTH);

        let forward_req = Request::from_parts(parts, body);

        // 根据协议选择客户端
        let is_https = target_url.starts_with("https://");
        let resp = if is_https {
            self.https_client
                .request(forward_req)
                .await
                .map_err(|e| MockproxyrsError::Http(e.to_string()))?
        } else {
            self.http_client
                .request(forward_req)
                .await
                .map_err(|e| MockproxyrsError::Http(e.to_string()))?
        };

        Ok(resp)
    }

    /// 从 URL 中提取 host
    fn extract_host(&self, url: &str) -> String {
        // 移除协议前缀
        let url = url
            .trim_start_matches("https://")
            .trim_start_matches("http://");

        // 移除路径部分
        url.split('/').next().unwrap_or(url).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_host() {
        let client = UpstreamClient::new();

        // HTTPS URL
        assert_eq!(client.extract_host("https://example.com"), "example.com");
        assert_eq!(
            client.extract_host("https://example.com:8080"),
            "example.com:8080"
        );
        assert_eq!(
            client.extract_host("https://example.com/api/users"),
            "example.com"
        );
        assert_eq!(
            client.extract_host("https://example.com:8080/api/users"),
            "example.com:8080"
        );

        // HTTP URL
        assert_eq!(client.extract_host("http://example.com"), "example.com");
        assert_eq!(
            client.extract_host("http://example.com:3000"),
            "example.com:3000"
        );
        assert_eq!(
            client.extract_host("http://example.com/api/users"),
            "example.com"
        );

        // Edge cases
        assert_eq!(client.extract_host("example.com"), "example.com");
        assert_eq!(client.extract_host("example.com/api"), "example.com");
    }

    #[test]
    fn test_upstream_client_default() {
        let _client = UpstreamClient::default();
    }

    #[test]
    fn test_upstream_client_clone() {
        let client = UpstreamClient::new();
        let _cloned = client.clone();
    }
}
