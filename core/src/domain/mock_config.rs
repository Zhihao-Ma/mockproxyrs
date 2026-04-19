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

//! Mock 配置领域模型

use serde::{Deserialize, Serialize};

/// Mock 服务配置（持久化）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockService {
    pub id: String,
    pub name: String,
    /// 监听地址，如 "127.0.0.1:8080"
    pub listen_addr: String,
    /// 目标地址，如 "https://example.com"
    pub target_url: String,
}

impl MockService {
    pub fn new(id: String, name: String, listen_addr: String, target_url: String) -> Self {
        Self {
            id,
            name,
            listen_addr,
            target_url,
        }
    }
}

/// Mock 服务详情（包含运行状态）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockServiceDetail {
    pub id: String,
    pub name: String,
    /// 监听地址，如 "127.0.0.1:8080"
    pub listen_addr: String,
    /// 目标地址，如 "https://example.com"
    pub target_url: String,
    /// 是否运行中
    pub running: bool,
}

impl MockServiceDetail {
    pub fn from_service(service: MockService, running: bool) -> Self {
        Self {
            id: service.id,
            name: service.name,
            listen_addr: service.listen_addr,
            target_url: service.target_url,
            running,
        }
    }
}

/// HTTP 请求方法
///
/// 用于规则匹配，ALL 表示匹配所有方法。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    /// 匹配所有方法
    #[default]
    All,
    /// GET 请求
    Get,
    /// POST 请求
    Post,
    /// PUT 请求
    Put,
    /// DELETE 请求
    Delete,
}

impl Method {
    /// 检查是否匹配指定方法
    ///
    /// - `All` 匹配任何方法
    /// - 其他值精确匹配
    pub fn matches(&self, method: &str) -> bool {
        match self {
            Method::All => true,
            Method::Get => method.eq_ignore_ascii_case("GET"),
            Method::Post => method.eq_ignore_ascii_case("POST"),
            Method::Put => method.eq_ignore_ascii_case("PUT"),
            Method::Delete => method.eq_ignore_ascii_case("DELETE"),
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Method::All => write!(f, "ALL"),
            Method::Get => write!(f, "GET"),
            Method::Post => write!(f, "POST"),
            Method::Put => write!(f, "PUT"),
            Method::Delete => write!(f, "DELETE"),
        }
    }
}

/// Mock 规则
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockRule {
    pub id: String,
    pub service_id: String,
    /// URL 匹配模式
    pub url_pattern: String,
    /// 是否为正则匹配（true=正则匹配，false=精确匹配）
    #[serde(default)]
    pub is_regex: bool,
    /// HTTP 方法（ALL 表示匹配所有方法）
    pub method: Method,
    /// 是否启用 mock
    pub enabled: bool,
    /// 是否转发并记录（true=转发请求并记录响应，false=直接返回 mock 响应）
    pub forward_and_record: bool,
    /// mock 响应内容
    pub mock_response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockRuleDTO {
    pub id: Option<String>,
    pub service_id: Option<String>,
    /// URL 匹配模式
    pub url_pattern: Option<String>,
    /// 是否为正则匹配（true=正则匹配，false=精确匹配）
    #[serde(default)]
    pub is_regex: Option<bool>,
    /// HTTP 方法（ALL 表示匹配所有方法）
    pub method: Option<String>,
    /// 是否启用 mock
    pub enabled: Option<bool>,
    /// 是否转发并记录（true=转发请求并记录响应，false=直接返回 mock 响应）
    pub forward_and_record: Option<bool>,
    /// mock 响应内容
    pub mock_response: Option<String>,
}

impl MockRule {
    pub fn new(
        id: String,
        service_id: String,
        url_pattern: String,
        is_regex: bool,
        method: Method,
        enabled: bool,
        forward_and_record: bool,
        mock_response: String,
    ) -> Self {
        Self {
            id,
            service_id,
            url_pattern,
            is_regex,
            method,
            enabled,
            forward_and_record,
            mock_response,
        }
    }
}

/// 服务运行状态（不持久化）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub service_id: String,
    pub running: bool,
    pub started_at: Option<i64>,
}

impl ServiceStatus {
    pub fn new(service_id: String, running: bool, started_at: Option<i64>) -> Self {
        Self {
            service_id,
            running,
            started_at,
        }
    }

    pub fn stopped(service_id: String) -> Self {
        Self {
            service_id,
            running: false,
            started_at: None,
        }
    }

    pub fn running(service_id: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .ok();
        Self {
            service_id,
            running: true,
            started_at: now,
        }
    }
}

/// 响应事件（推送到前端）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResponseEvent {
    pub service_id: String,
    pub service_name: String,
    /// 请求 URL
    pub url: String,
    /// 匹配的URL
    pub url_pattern: Option<String>,
    /// 请求方法
    pub method: String,
    /// 正则匹配
    pub is_regex: bool,
    /// 匹配的规则 ID
    pub matched_rule_id: Option<String>,
    /// 是否命中 mock
    pub is_mock: bool,
    /// 是否转发了请求
    pub forwarded: bool,
    /// 实际响应内容
    pub response_body: String,
    /// mock 响应内容（如果有）
    pub mock_body: Option<String>,
    /// 时间戳（毫秒）
    pub timestamp: i64,
}

impl ResponseEvent {
    pub fn new(
        service_id: String,
        service_name: String,
        url: String,
        url_pattern: Option<String>,
        method: String,
        is_regex: bool,
        matched_rule_id: Option<String>,
        is_mock: bool,
        forwarded: bool,
        response_body: String,
        mock_body: Option<String>,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self {
            service_id,
            service_name,
            url,
            url_pattern,
            method,
            is_regex,
            matched_rule_id,
            is_mock,
            forwarded,
            response_body,
            mock_body,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_matches() {
        // ALL matches everything
        assert!(Method::All.matches("GET"));
        assert!(Method::All.matches("POST"));
        assert!(Method::All.matches("PUT"));
        assert!(Method::All.matches("DELETE"));
        assert!(Method::All.matches("PATCH"));
        assert!(Method::All.matches("any"));

        // GET matches only GET (case insensitive)
        assert!(Method::Get.matches("GET"));
        assert!(Method::Get.matches("get"));
        assert!(Method::Get.matches("Get"));
        assert!(!Method::Get.matches("POST"));
        assert!(!Method::Get.matches("PUT"));

        // POST matches only POST
        assert!(Method::Post.matches("POST"));
        assert!(Method::Post.matches("post"));
        assert!(!Method::Post.matches("GET"));

        // PUT matches only PUT
        assert!(Method::Put.matches("PUT"));
        assert!(Method::Put.matches("put"));
        assert!(!Method::Put.matches("POST"));

        // DELETE matches only DELETE
        assert!(Method::Delete.matches("DELETE"));
        assert!(Method::Delete.matches("delete"));
        assert!(!Method::Delete.matches("GET"));
    }

    #[test]
    fn test_method_default() {
        assert_eq!(Method::default(), Method::All);
    }

    #[test]
    fn test_mock_service_new() {
        let service = MockService::new(
            "svc-1".to_string(),
            "Test Service".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );

        assert_eq!(service.id, "svc-1");
        assert_eq!(service.name, "Test Service");
        assert_eq!(service.listen_addr, "127.0.0.1:8080");
        assert_eq!(service.target_url, "https://example.com");
    }

    #[test]
    fn test_mock_service_detail_from_service() {
        let service = MockService::new(
            "svc-1".to_string(),
            "Test Service".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );

        let detail = MockServiceDetail::from_service(service.clone(), true);
        assert_eq!(detail.id, service.id);
        assert_eq!(detail.name, service.name);
        assert_eq!(detail.listen_addr, service.listen_addr);
        assert_eq!(detail.target_url, service.target_url);
        assert!(detail.running);

        let detail = MockServiceDetail::from_service(service, false);
        assert!(!detail.running);
    }

    #[test]
    fn test_mock_rule_new() {
        let rule = MockRule::new(
            "rule-1".to_string(),
            "svc-1".to_string(),
            "/api/users".to_string(),
            false,
            Method::Get,
            true,
            false,
            r#"{"code": 200}"#.to_string(),
        );

        assert_eq!(rule.id, "rule-1");
        assert_eq!(rule.service_id, "svc-1");
        assert_eq!(rule.url_pattern, "/api/users");
        assert!(!rule.is_regex);
        assert_eq!(rule.method, Method::Get);
        assert!(rule.enabled);
        assert!(!rule.forward_and_record);
        assert_eq!(rule.mock_response, r#"{"code": 200}"#);
    }

    #[test]
    fn test_service_status_stopped() {
        let status = ServiceStatus::stopped("svc-1".to_string());
        assert_eq!(status.service_id, "svc-1");
        assert!(!status.running);
        assert!(status.started_at.is_none());
    }

    #[test]
    fn test_service_status_running() {
        let status = ServiceStatus::running("svc-1".to_string());
        assert_eq!(status.service_id, "svc-1");
        assert!(status.running);
        assert!(status.started_at.is_some());
        // started_at should be close to current time
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap();
        let diff = (status.started_at.unwrap() - now).abs();
        assert!(diff < 2, "started_at should be within 2 seconds of now");
    }

    #[test]
    fn test_response_event_new() {
        let event = ResponseEvent::new(
            "svc-1".to_string(),
            "Test Service".to_string(),
            "/api/users".to_string(),
            Some("/api/users".to_string()),
            "ALL".to_string(),
            false,
            Some("rule-1".to_string()),
            true,
            false,
            r#"{"code": 200}"#.to_string(),
            Some(r#"{"mock": true}"#.to_string()),
        );

        assert_eq!(event.service_id, "svc-1");
        assert_eq!(event.service_name, "Test Service");
        assert_eq!(event.url, "/api/users");
        assert_eq!(event.matched_rule_id, Some("rule-1".to_string()));
        assert!(event.is_mock);
        assert!(!event.forwarded);
        assert_eq!(event.response_body, r#"{"code": 200}"#);
        assert_eq!(event.mock_body, Some(r#"{"mock": true}"#.to_string()));
        assert!(event.timestamp > 0);
    }

    #[test]
    fn test_response_event_without_mock() {
        let event = ResponseEvent::new(
            "svc-1".to_string(),
            "Test Service".to_string(),
            "/api/users".to_string(),
            Some("/api/users".to_string()),
            "ALL".to_string(),
            false,
            None,
            false,
            true,
            "response".to_string(),
            None,
        );

        assert!(event.matched_rule_id.is_none());
        assert!(!event.is_mock);
        assert!(event.forwarded);
        assert!(event.mock_body.is_none());
    }

    #[test]
    fn test_method_serde() {
        // Test serialization
        let json = serde_json::to_string(&Method::Get).unwrap();
        assert_eq!(json, r#""GET""#);

        let json = serde_json::to_string(&Method::All).unwrap();
        assert_eq!(json, r#""ALL""#);

        // Test deserialization
        let method: Method = serde_json::from_str(r#""POST""#).unwrap();
        assert_eq!(method, Method::Post);

        // Note: serde rename_all = "UPPERCASE" requires uppercase input
        let method: Method = serde_json::from_str(r#""PUT""#).unwrap();
        assert_eq!(method, Method::Put);

        let method: Method = serde_json::from_str(r#""DELETE""#).unwrap();
        assert_eq!(method, Method::Delete);

        let method: Method = serde_json::from_str(r#""ALL""#).unwrap();
        assert_eq!(method, Method::All);
    }

    #[test]
    fn test_mock_service_serde() {
        let service = MockService::new(
            "svc-1".to_string(),
            "Test Service".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );

        let json = serde_json::to_string(&service).unwrap();
        let parsed: MockService = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, service);
    }

    #[test]
    fn test_mock_rule_serde() {
        let rule = MockRule::new(
            "rule-1".to_string(),
            "svc-1".to_string(),
            "/api/users".to_string(),
            true,
            Method::Post,
            true,
            true,
            r#"{"code": 201}"#.to_string(),
        );

        let json = serde_json::to_string(&rule).unwrap();
        let parsed: MockRule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rule);
    }
}
