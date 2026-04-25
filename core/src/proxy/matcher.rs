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

//! 规则匹配器
//!
//! 根据请求 URL 和 HTTP 方法匹配对应的 Mock 规则。
//!
//! # 匹配规则
//!
//! - **方法匹配**: 规则方法为 ALL 或与请求方法一致
//! - **精确匹配**: `is_regex=false` 时，URL 完全匹配规则模式
//! - **正则匹配**: `is_regex=true` 时，URL 匹配正则表达式
//!
//! # 性能优化
//!
//! - 精确匹配使用 HashMap 快速查找，O(1) 时间复杂度
//! - 正则匹配需要遍历，O(n) 时间复杂度
//!
//! # 示例
//!
//! - `/api/users` + GET (is_regex=false) 精确匹配 GET `/api/users`
//! - `/api/.*` + ALL (is_regex=true) 正则匹配所有 `/api/` 开头的路径

use regex::Regex;
use std::collections::HashMap;

use crate::domain::{Method, MockRule};

/// 编译后的规则，用于高效匹配
#[derive(Debug, Clone)]
pub struct CompiledRule {
    /// 原始规则引用的 ID
    pub rule_id: String,
    /// 编译后的正则表达式（仅当 is_regex=true 时有效）
    pub regex: Option<Regex>,
}

/// 规则匹配器
///
/// 维护两个索引：
/// - `exact_match_map`: 精确匹配的 URL -> 规则 ID 映射
/// - `regex_rules`: 正则匹配规则列表
#[derive(Debug, Clone, Default)]
pub struct RuleMatcher {
    /// 精确匹配映射：URL -> (规则ID, 方法)
    pub exact_match_map: HashMap<(String, String), String>,
    /// 正则匹配规则列表
    pub regex_rules: HashMap<(String, String), CompiledRule>,
}

impl RuleMatcher {
    /// 创建空的匹配器
    pub fn new() -> Self {
        Self::default()
    }

    /// 从规则列表构建匹配器索引
    ///
    /// # Arguments
    /// * `rules` - 规则列表
    ///
    /// # Returns
    /// 构建好的匹配器，包含精确匹配 HashMap 和正则匹配列表
    pub fn build(rules: &HashMap<String, MockRule>) -> Self {
        let mut exact_match_map = HashMap::new();
        let mut regex_rules = HashMap::new();

        for rule in rules.values() {
            if rule.is_regex {
                // 正则匹配规则
                if let Ok(regex) = Regex::new(&rule.url_pattern) {
                    regex_rules.insert(
                        (rule.url_pattern.clone(), rule.method.to_string()),
                        CompiledRule {
                            rule_id: rule.id.clone(),
                            regex: Some(regex),
                        },
                    );
                }
            } else {
                // 精确匹配规则
                exact_match_map.insert(
                    (rule.url_pattern.clone(), rule.method.to_string()),
                    rule.id.clone(),
                );
            }
        }

        Self {
            exact_match_map,
            regex_rules,
        }
    }

    /// 匹配 URL 和方法对应的规则
    ///
    /// # Arguments
    /// * `method` - HTTP 方法（如 "GET", "POST"）
    /// * `url` - 请求 URL（不含域名，如 "/api/users?id=1"）
    /// * `rules` - 规则列表
    ///
    /// # Returns
    /// 匹配到的规则，如果没有匹配则返回 None
    ///
    /// # 匹配顺序
    /// 1. 先尝试精确匹配（HashMap O(1) 查找）
    /// 2. 再尝试正则匹配（遍历 O(n)）
    pub fn match_rule<'a>(
        &self,
        method: &str,
        url: &str,
        rules: &'a HashMap<String, MockRule>,
    ) -> Option<&'a MockRule> {
        // 提取 URL 路径（不含查询参数）
        let path = url.split('?').next().unwrap_or(url);

        // 1. 先尝试精确匹配（快速路径）
        let rule_opt = self
            .exact_match_map
            .get(&(path.to_string(), method.to_string()));
        let rule_opt = if rule_opt.is_some() {
            rule_opt
        } else {
            self.exact_match_map
                .get(&(path.to_string(), Method::All.to_string()))
        };
        if let Some(rule_id) = rule_opt
            && let Some(rule) = rules.get(rule_id)
        {
            return Some(rule);
        }
        // 也尝试完整 URL（包含查询参数）的精确匹配
        if path != url {
            let rule_opt = self
                .exact_match_map
                .get(&(url.to_string(), method.to_string()));
            let rule_opt = if rule_opt.is_some() {
                rule_opt
            } else {
                self.exact_match_map
                    .get(&(url.to_string(), Method::All.to_string()))
            };
            if let Some(rule_id) = rule_opt
                && let Some(rule) = rules.get(rule_id)
            {
                return Some(rule);
            }
        }

        // 2. 再尝试正则匹配
        for compiled in self.regex_rules.values() {
            if let Some(rule) = rules.get(&compiled.rule_id) {
                if !rule.method.matches(method) {
                    continue;
                }

                if let Some(ref regex) = compiled.regex
                    && regex.is_match(path)
                {
                    return Some(rule);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Method;

    fn create_rule(
        id: &str,
        pattern: &str,
        is_regex: bool,
        method: Method,
        enabled: bool,
    ) -> MockRule {
        MockRule::new(
            id.to_string(),
            "service-1".to_string(),
            pattern.to_string(),
            is_regex,
            method,
            enabled,
            false,
            "{}".to_string(),
        )
    }

    #[test]
    fn test_exact_match() {
        let mut rules = HashMap::new();
        rules.insert(
            "rule-1".to_string(),
            create_rule("rule-1", "/api/users", false, Method::Get, true),
        );

        let matcher = RuleMatcher::build(&rules);

        let result = matcher.match_rule("GET", "/api/users", &rules);
        assert!(result.is_some());

        let result = matcher.match_rule("GET", "/api/users?id=1", &rules);
        assert!(result.is_some());

        let result = matcher.match_rule("GET", "/api/posts", &rules);
        assert!(result.is_none());
    }

    #[test]
    fn test_regex_match() {
        let mut rules = HashMap::new();
        rules.insert(
            "rule-1".to_string(),
            create_rule("rule-1", r"/api/.*", true, Method::All, true),
        );

        let matcher = RuleMatcher::build(&rules);

        let result = matcher.match_rule("GET", "/api/users", &rules);
        assert!(result.is_some());

        let result = matcher.match_rule("POST", "/api/posts/123", &rules);
        assert!(result.is_some());

        let result = matcher.match_rule("GET", "/other", &rules);
        assert!(result.is_none());
    }

    #[test]
    fn test_method_match() {
        let mut rules = HashMap::new();
        rules.insert(
            "rule-1".to_string(),
            create_rule("rule-1", "/api/users", false, Method::Post, true),
        );

        let matcher = RuleMatcher::build(&rules);

        // POST 匹配
        let result = matcher.match_rule("POST", "/api/users", &rules);
        assert!(result.is_some());

        // GET 不匹配
        let result = matcher.match_rule("GET", "/api/users", &rules);
        assert!(result.is_none());
    }

    #[test]
    fn test_all_method() {
        let mut rules = HashMap::new();
        rules.insert(
            "rule-1".to_string(),
            create_rule("rule-1", "/api/users", false, Method::All, true),
        );

        let matcher = RuleMatcher::build(&rules);

        // ALL 匹配所有方法
        assert!(matcher.match_rule("GET", "/api/users", &rules).is_some());
        assert!(matcher.match_rule("POST", "/api/users", &rules).is_some());
        assert!(matcher.match_rule("PUT", "/api/users", &rules).is_some());
        assert!(matcher.match_rule("DELETE", "/api/users", &rules).is_some());
    }

    #[test]
    fn test_disabled_rule() {
        let mut rules = HashMap::new();
        rules.insert(
            "rule-1".to_string(),
            create_rule("rule-1", "/api/users", false, Method::Get, false),
        );

        let matcher = RuleMatcher::build(&rules);

        let result = matcher.match_rule("GET", "/api/users", &rules);
        assert!(result.is_some());
    }

    #[test]
    fn test_exact_match_priority() {
        // 精确匹配应该优先于正则匹配
        let mut rules = HashMap::new();
        rules.insert(
            "rule-exact".to_string(),
            create_rule("rule-exact", "/api/users", false, Method::Get, true),
        );
        rules.insert(
            "rule-regex".to_string(),
            create_rule("rule-regex", r"/api/.*", true, Method::Get, true),
        );

        let matcher = RuleMatcher::build(&rules);

        let result = matcher.match_rule("GET", "/api/users", &rules);
        assert!(result.is_some());
        // 应该匹配精确匹配的规则
        assert_eq!(result.unwrap().id, "rule-exact");
    }

    #[test]
    fn test_exact_match_method_priority() {
        // 对应请求方法优先于ALL
        let mut rules = HashMap::new();
        rules.insert(
            "rule-get".to_string(),
            create_rule("rule-get", "/api/users", false, Method::Get, true),
        );
        rules.insert(
            "rule-all".to_string(),
            create_rule("rule-all", r"/api/users", false, Method::All, true),
        );

        let matcher = RuleMatcher::build(&rules);

        let result = matcher.match_rule("GET", "/api/users", &rules);
        assert!(result.is_some());
        // 应该匹配精确匹配的规则
        assert_eq!(result.unwrap().id, "rule-get");
    }

    #[test]
    fn test_regex_pattern() {
        let mut rules = HashMap::new();
        // 匹配 /api/users/ 后面跟数字
        rules.insert(
            "rule-1".to_string(),
            create_rule("rule-1", r"/api/users/\d+", true, Method::Get, true),
        );

        let matcher = RuleMatcher::build(&rules);

        let result = matcher.match_rule("GET", "/api/users/123", &rules);
        assert!(result.is_some());

        let result = matcher.match_rule("GET", "/api/users/abc", &rules);
        assert!(result.is_none());
    }
}
