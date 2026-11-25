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

//! 规则与服务的统一存取入口
//!
//! 把「写入仓库 + 同步到运行中的代理服务」收敛到一处，
//! 供桌面端 IPC 命令与 MCP 工具共用，保证所有入口的规则变更语义一致：
//! 变更落库后立即对运行中的服务生效，无需重启服务。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::domain::{MockRule, MockService};
use crate::error::{MockproxyrsError, Result};
use crate::proxy::ProxyServer;
use crate::repository::MockRepository;

/// 规则与服务的统一存取入口
#[derive(Clone)]
pub struct RuleStore {
    repository: Arc<dyn MockRepository>,
    /// 运行中的代理服务（服务 ID -> 代理服务器）
    services: Arc<RwLock<HashMap<String, Arc<ProxyServer>>>>,
}

impl RuleStore {
    /// 创建 RuleStore
    pub fn new(
        repository: Arc<dyn MockRepository>,
        services: Arc<RwLock<HashMap<String, Arc<ProxyServer>>>>,
    ) -> Self {
        Self {
            repository,
            services,
        }
    }

    /// 底层仓库（需要仓库特有能力时使用）
    pub fn repository(&self) -> &Arc<dyn MockRepository> {
        &self.repository
    }

    /// 运行中的代理服务表
    pub fn running_services(&self) -> &Arc<RwLock<HashMap<String, Arc<ProxyServer>>>> {
        &self.services
    }

    /// 获取所有服务
    pub async fn list_services(&self) -> Result<Vec<MockService>> {
        self.repository.list_services().await
    }

    /// 检查服务是否运行中
    pub async fn is_service_running(&self, id: &str) -> bool {
        self.services.read().await.contains_key(id)
    }

    /// 获取服务的所有规则
    pub async fn list_rules(&self, service_id: &str) -> Result<Vec<MockRule>> {
        self.repository.list_rules(service_id).await
    }

    /// 根据 ID 获取规则，不存在时返回 [`MockproxyrsError::NotFound`]
    pub async fn get_rule(&self, id: &str) -> Result<MockRule> {
        self.repository
            .get_rule(id)
            .await?
            .ok_or_else(|| MockproxyrsError::NotFound(format!("规则不存在: {}", id)))
    }

    /// 保存规则（新增或更新）
    ///
    /// 先写入仓库，再同步到运行中的服务。`ProxyServer::update_rule` 对不存在的新规则
    /// 同样适用（无旧条目需要清理），因此新增与更新共用同一条同步路径。
    /// 变体的 `rule_id` 统一归一化为所属规则的 id，客户端无需维护这个字段。
    pub async fn save_rule(&self, mut rule: MockRule) -> Result<()> {
        for variant in &mut rule.response_variants {
            variant.rule_id = rule.id.clone();
        }

        self.repository.save_rule(&rule).await?;

        let services = self.services.read().await;
        if let Some(running) = services.get(&rule.service_id) {
            running.update_rule(rule.clone()).await;
        }

        Ok(())
    }

    /// 删除规则
    ///
    /// 先写入仓库，再同步到运行中的服务；规则不存在时返回 [`MockproxyrsError::NotFound`]。
    pub async fn delete_rule(&self, id: &str) -> Result<()> {
        let rule = self.get_rule(id).await?;

        self.repository.delete_rule(id).await?;

        let services = self.services.read().await;
        if let Some(running) = services.get(&rule.service_id) {
            running.delete_rule(id).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Method, MockResponseVariant};
    use crate::event::NoopEmitter;
    use crate::mock::ScriptEngine;
    use crate::proxy::RuleMatcher;
    use crate::repository::{MockRepository, SqliteRepository};
    use tokio::sync::watch;
    const SERVICE_ID: &str = "svc-1";

    /// 内存仓库 + 一个未启动但持有规则表/匹配器的 ProxyServer（足以验证同步逻辑）
    async fn setup() -> (RuleStore, Arc<SqliteRepository>, Arc<ProxyServer>) {
        let repo = Arc::new(SqliteRepository::in_memory().unwrap());
        let config = MockService::new(
            SERVICE_ID.into(),
            "Test".into(),
            "127.0.0.1:0".into(),
            "http://example.com".into(),
        );
        repo.save_service(&config).await.unwrap();

        let rules = Arc::new(RwLock::new(HashMap::new()));
        let matcher = Arc::new(RwLock::new(RuleMatcher::build(&*rules.read().await)));
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let server = Arc::new(ProxyServer::new(
            config,
            rules,
            Arc::new(NoopEmitter),
            matcher,
            Arc::new(ScriptEngine::new().unwrap()),
            shutdown_tx,
        ));
        let services = Arc::new(RwLock::new(HashMap::from([(
            SERVICE_ID.to_string(),
            server.clone(),
        )])));

        (
            RuleStore::new(repo.clone() as Arc<dyn MockRepository>, services),
            repo,
            server,
        )
    }

    fn sample_rule(id: &str, service_id: &str, pattern: &str) -> MockRule {
        MockRule::new(
            id.into(),
            service_id.into(),
            pattern.into(),
            false,
            Method::Get,
            true,
            false,
            "{}".into(),
            None,
            None,
            false,
        )
    }

    /// 运行中服务里匹配到某条规则，返回其 id
    async fn matched_id(server: &ProxyServer, url: &str) -> Option<String> {
        let rules = server.rules.read().await;
        let matcher = server.matcher.read().await;
        matcher.match_rule("GET", url, &rules).map(|r| r.id.clone())
    }

    #[tokio::test]
    async fn test_save_rule_persists_and_syncs_to_running_server() {
        let (store, repo, server) = setup().await;
        let rule = sample_rule("r1", SERVICE_ID, "/api/x");
        store.save_rule(rule.clone()).await.unwrap();

        assert_eq!(repo.get_rule("r1").await.unwrap(), Some(rule.clone()));
        assert_eq!(
            server.rules.read().await.get("r1"),
            Some(&rule),
            "规则表已同步"
        );
        assert_eq!(
            matched_id(&server, "/api/x").await.as_deref(),
            Some("r1"),
            "匹配器索引已同步，无需重启即可命中"
        );

        // 更新 URL 模式：旧索引被清理，新索引生效
        let mut moved = rule.clone();
        moved.url_pattern = "/api/y".into();
        store.save_rule(moved.clone()).await.unwrap();
        assert_eq!(matched_id(&server, "/api/x").await, None, "旧模式不再命中");
        assert_eq!(matched_id(&server, "/api/y").await.as_deref(), Some("r1"));
        assert_eq!(repo.get_rule("r1").await.unwrap(), Some(moved));
    }

    #[tokio::test]
    async fn test_delete_rule_removes_from_store_and_running_server() {
        let (store, repo, server) = setup().await;
        store
            .save_rule(sample_rule("r1", SERVICE_ID, "/api/x"))
            .await
            .unwrap();

        store.delete_rule("r1").await.unwrap();

        assert_eq!(repo.get_rule("r1").await.unwrap(), None);
        assert!(server.rules.read().await.get("r1").is_none());
        assert_eq!(matched_id(&server, "/api/x").await, None);

        // 重复删除：规则不存在 → NotFound
        assert!(matches!(
            store.delete_rule("r1").await,
            Err(MockproxyrsError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_get_rule_missing_is_not_found() {
        let (store, _, _) = setup().await;
        assert!(matches!(
            store.get_rule("missing").await,
            Err(MockproxyrsError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_save_rule_normalizes_variant_rule_id() {
        let (store, repo, _) = setup().await;
        let mut rule = sample_rule("r1", SERVICE_ID, "/api/x");
        rule.response_variants = vec![MockResponseVariant {
            id: "v1".into(),
            rule_id: "wrong".into(),
            name: "成功".into(),
            mock_response: "{}".into(),
        }];
        rule.selected_variant_id = Some("v1".into());

        store.save_rule(rule.clone()).await.unwrap();

        rule.response_variants[0].rule_id = "r1".into();
        assert_eq!(repo.get_rule("r1").await.unwrap(), Some(rule));
    }

    #[tokio::test]
    async fn test_save_rule_for_stopped_service_only_persists() {
        let (store, repo, _) = setup().await;
        // svc-2 没有运行中的服务：只落库，不报错
        repo.save_service(&MockService::new(
            "svc-2".into(),
            "Other".into(),
            "127.0.0.1:0".into(),
            "http://example.com".into(),
        ))
        .await
        .unwrap();

        store
            .save_rule(sample_rule("r2", "svc-2", "/api/z"))
            .await
            .unwrap();

        assert!(repo.get_rule("r2").await.unwrap().is_some());
        assert!(!store.is_service_running("svc-2").await);
        assert!(store.is_service_running(SERVICE_ID).await);
    }
}
