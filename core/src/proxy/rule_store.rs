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
//! 把「写入仓库 + 同步到运行中的代理服务 + 推送变更事件」收敛到一处，
//! 供桌面端 IPC 命令、MCP 工具与将来的 web 端共用，保证所有入口的规则变更语义一致：
//! 变更落库后立即对运行中的服务生效，无需重启服务；其它页面同时也能收到通知并局部刷新。
//!
//! 写入粒度：规则字段与响应变体是两个独立的写入维度。保存规则只写规则自身的列，
//! 对变体的增删改/切换激活是行级操作——两者互不覆盖，是多页面同时编辑时不丢数据的前提。

use std::collections::HashMap;
use std::sync::Arc;

use log::error;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::domain::{MockResponseVariant, MockRule, MockService, ResponseKind};
use crate::error::{MockproxyrsError, Result};
use crate::event::{RuleChangeEmitter, RuleChangeEvent};
use crate::proxy::ProxyServer;
use crate::repository::MockRepository;

/// 规则与服务的统一存取入口
#[derive(Clone)]
pub struct RuleStore {
    repository: Arc<dyn MockRepository>,
    /// 运行中的代理服务（服务 ID -> 代理服务器）
    services: Arc<RwLock<HashMap<String, Arc<ProxyServer>>>>,
    /// 规则变更事件发射器
    rule_changes: Arc<dyn RuleChangeEmitter>,
}

impl RuleStore {
    /// 创建 RuleStore
    pub fn new(
        repository: Arc<dyn MockRepository>,
        services: Arc<RwLock<HashMap<String, Arc<ProxyServer>>>>,
        rule_changes: Arc<dyn RuleChangeEmitter>,
    ) -> Self {
        Self {
            repository,
            services,
            rule_changes,
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

    /// 新增规则并返回仓库中的完整规则（含空的响应集）
    ///
    /// 传入规则的 `response_variants` / `selected_variant_id` 会被忽略：新规则没有变体，
    /// 变体请用 [`RuleStore::add_variant`] 追加。
    pub async fn save_rule(&self, rule: MockRule) -> Result<MockRule> {
        self.repository.save_rule(&rule).await?;
        self.publish(rule.id).await
    }

    /// 更新规则自身的字段，不触碰响应变体与激活项
    ///
    /// `patch` 在仓库当前状态上修改，只应改动传入调用方明确要改的字段；返回更新后的完整规则。
    /// 规则不存在时返回 [`MockproxyrsError::NotFound`]。
    pub async fn update_rule(
        &self,
        id: &str,
        patch: impl FnOnce(&mut MockRule),
    ) -> Result<MockRule> {
        let mut rule = self.get_rule(id).await?;
        patch(&mut rule);
        // 无论 patch 是否动了变体字段，落库的都只有规则自身的列（仓库层保证）
        self.repository.save_rule(&rule).await?;
        self.publish(id.to_string()).await
    }

    /// 删除规则
    ///
    /// 先写入仓库，再同步到运行中的服务，最后推送删除事件；规则不存在时返回
    /// [`MockproxyrsError::NotFound`]。
    pub async fn delete_rule(&self, id: &str) -> Result<()> {
        let rule = self.get_rule(id).await?;

        self.repository.delete_rule(id).await?;

        let services = self.services.read().await;
        if let Some(running) = services.get(&rule.service_id) {
            running.delete_rule(id).await;
        }
        drop(services);

        self.emit(RuleChangeEvent::Deleted {
            rule_id: id.to_string(),
            service_id: rule.service_id,
        })
        .await;

        Ok(())
    }

    // ==================== 响应变体（行级操作） ====================

    /// 新增响应变体（指定类型），追加到响应集末尾；`activate` 为 true 时同时切换为激活项
    pub async fn add_variant(
        &self,
        rule_id: &str,
        name: &str,
        kind: ResponseKind,
        content: &str,
        activate: bool,
    ) -> Result<MockRule> {
        self.get_rule(rule_id).await?;

        let variant = MockResponseVariant {
            id: Uuid::new_v4().to_string(),
            rule_id: rule_id.to_string(),
            name: name.to_string(),
            kind,
            content: content.to_string(),
        };
        self.repository.add_variant(&variant).await?;
        if activate {
            self.repository
                .set_selected_variant(rule_id, Some(&variant.id))
                .await?;
        }

        self.publish(rule_id.to_string()).await
    }

    /// 重命名响应变体
    pub async fn rename_variant(
        &self,
        rule_id: &str,
        variant_id: &str,
        name: &str,
    ) -> Result<MockRule> {
        self.repository.rename_variant(variant_id, name).await?;
        self.publish(rule_id.to_string()).await
    }

    /// 更新响应变体的类型与内容（单一内容槽：切类型的同时内容被替换）
    pub async fn update_variant(
        &self,
        rule_id: &str,
        variant_id: &str,
        kind: ResponseKind,
        content: &str,
    ) -> Result<MockRule> {
        self.repository
            .update_variant(variant_id, kind, content)
            .await?;
        self.publish(rule_id.to_string()).await
    }

    /// 删除响应变体；若删的是激活项则回退到规则本身的 mock_response
    pub async fn delete_variant(&self, rule_id: &str, variant_id: &str) -> Result<MockRule> {
        self.repository.delete_variant(variant_id).await?;
        self.publish(rule_id.to_string()).await
    }

    /// 切换规则当前激活的响应变体；`None` 表示取消激活，回退到规则本身的 mock_response
    pub async fn set_selected_variant(
        &self,
        rule_id: &str,
        variant_id: Option<&str>,
    ) -> Result<MockRule> {
        self.repository
            .set_selected_variant(rule_id, variant_id)
            .await?;
        self.publish(rule_id.to_string()).await
    }

    // ==================== 内部 ====================

    /// 从仓库重载完整规则，同步到运行中的服务（变体与激活项一并生效），推送更新事件
    async fn publish(&self, rule_id: String) -> Result<MockRule> {
        let rule = self.get_rule(&rule_id).await?;

        let services = self.services.read().await;
        if let Some(running) = services.get(&rule.service_id) {
            running.update_rule(rule.clone()).await;
        }
        drop(services);

        self.emit(RuleChangeEvent::Updated { rule: rule.clone() })
            .await;

        Ok(rule)
    }

    /// 发送事件；推送失败只记日志，不影响已落库的变更
    async fn emit(&self, event: RuleChangeEvent) {
        if let Err(e) = self.rule_changes.emit_rule_change(event).await {
            error!("Error emitting rule change event: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Method;
    use crate::event::NoopEmitter;
    use crate::mock::ScriptEngine;
    use crate::proxy::RuleMatcher;
    use crate::repository::{MockRepository, SqliteRepository};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::watch;
    const SERVICE_ID: &str = "svc-1";

    /// 记录所有推送事件的发射器，用于断言
    #[derive(Clone, Default)]
    struct RecordingEmitter {
        events: Arc<StdMutex<Vec<RuleChangeEvent>>>,
    }

    impl RuleChangeEmitter for RecordingEmitter {
        fn emit_rule_change(
            &self,
            event: RuleChangeEvent,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.events.lock().unwrap().push(event);
            Box::pin(async { Ok(()) })
        }
    }

    /// 内存仓库 + 一个未启动但持有规则表/匹配器的 ProxyServer（足以验证同步逻辑）
    async fn setup() -> (
        RuleStore,
        Arc<SqliteRepository>,
        Arc<ProxyServer>,
        Arc<StdMutex<Vec<RuleChangeEvent>>>,
    ) {
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

        let events = Arc::new(StdMutex::new(Vec::new()));
        let store = RuleStore::new(
            repo.clone() as Arc<dyn MockRepository>,
            services,
            Arc::new(RecordingEmitter {
                events: events.clone(),
            }),
        );

        (store, repo, server, events)
    }

    /// 取出目前记录的事件并清空
    fn take_events(events: &Arc<StdMutex<Vec<RuleChangeEvent>>>) -> Vec<RuleChangeEvent> {
        std::mem::take(&mut *events.lock().unwrap())
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
        let (store, repo, server, events) = setup().await;
        let rule = sample_rule("r1", SERVICE_ID, "/api/x");
        let saved = store.save_rule(rule.clone()).await.unwrap();

        assert_eq!(saved, rule);
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
        let pushed = take_events(&events);
        assert_eq!(
            pushed,
            vec![RuleChangeEvent::Updated { rule: rule.clone() }],
            "新增后推送 Updated 事件"
        );

        // 更新 URL 模式：旧索引被清理，新索引生效
        store
            .update_rule("r1", |r| r.url_pattern = "/api/y".into())
            .await
            .unwrap();
        let moved = repo.get_rule("r1").await.unwrap().unwrap();
        assert_eq!(moved.url_pattern, "/api/y");
        assert_eq!(matched_id(&server, "/api/x").await, None, "旧模式不再命中");
        assert_eq!(matched_id(&server, "/api/y").await.as_deref(), Some("r1"));
        assert_eq!(
            take_events(&events),
            vec![RuleChangeEvent::Updated { rule: moved }],
            "更新后推送 Updated 事件"
        );

        assert!(matches!(
            store.update_rule("missing", |_| {}).await,
            Err(MockproxyrsError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_delete_rule_removes_from_store_and_running_server() {
        let (store, repo, server, events) = setup().await;
        store
            .save_rule(sample_rule("r1", SERVICE_ID, "/api/x"))
            .await
            .unwrap();
        take_events(&events);

        store.delete_rule("r1").await.unwrap();

        assert_eq!(repo.get_rule("r1").await.unwrap(), None);
        assert!(server.rules.read().await.get("r1").is_none());
        assert_eq!(matched_id(&server, "/api/x").await, None);
        assert_eq!(
            take_events(&events),
            vec![RuleChangeEvent::Deleted {
                rule_id: "r1".into(),
                service_id: SERVICE_ID.into(),
            }],
            "删除后推送 Deleted 事件"
        );

        // 重复删除：规则不存在 → NotFound
        assert!(matches!(
            store.delete_rule("r1").await,
            Err(MockproxyrsError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_get_rule_missing_is_not_found() {
        let (store, _, _, _) = setup().await;
        assert!(matches!(
            store.get_rule("missing").await,
            Err(MockproxyrsError::NotFound(_))
        ));
    }

    /// 核心场景：另一端加了变体后，本端用旧快照保存规则（经 update_rule），变体与激活项都保留
    #[tokio::test]
    async fn test_variant_ops_sync_to_running_server_and_survive_rule_update() {
        let (store, repo, server, events) = setup().await;
        store
            .save_rule(sample_rule("r1", SERVICE_ID, "/api/x"))
            .await
            .unwrap();
        take_events(&events);

        // 模拟另一端（如 MCP）加了两个变体（一静态一脚本）并激活脚本那个
        let rule = store
            .add_variant("r1", "成功", ResponseKind::Static, "{\"ok\":true}", true)
            .await
            .unwrap();
        assert_eq!(rule.response_variants.len(), 1);
        let v1 = rule.response_variants[0].id.clone();
        assert_eq!(rule.selected_variant_id.as_deref(), Some(v1.as_str()));
        let rule = store
            .add_variant(
                "r1",
                "失败",
                ResponseKind::Script,
                "return { status: 500, body: 'nope' };",
                false,
            )
            .await
            .unwrap();
        let v2 = rule.response_variants[1].id.clone();
        let rule = store.set_selected_variant("r1", Some(&v2)).await.unwrap();
        assert_eq!(rule.selected_variant_id.as_deref(), Some(v2.as_str()));
        assert_eq!(
            server
                .rules
                .read()
                .await
                .get("r1")
                .unwrap()
                .effective_response(),
            (
                ResponseKind::Script,
                "return { status: 500, body: 'nope' };".into()
            ),
            "激活切换（含类型）立即对运行中的服务生效"
        );
        assert_eq!(take_events(&events).len(), 3, "每次变体操作都推送一次");

        // 本端用旧快照改 URL：没有任何变体、未选中
        store
            .update_rule("r1", |r| {
                r.url_pattern = "/api/y".into();
                r.response_variants.clear();
                r.selected_variant_id = None;
            })
            .await
            .unwrap();

        let got = repo.get_rule("r1").await.unwrap().unwrap();
        assert_eq!(got.url_pattern, "/api/y", "规则字段正常更新");
        assert_eq!(got.response_variants.len(), 2, "另一端加的变体不受影响");
        assert_eq!(
            got.selected_variant_id.as_deref(),
            Some(v2.as_str()),
            "激活项不受影响"
        );
        let running = server.rules.read().await.get("r1").unwrap().clone();
        assert_eq!(running, got, "运行中服务拿到的是仓库里的最新完整规则");

        // 重命名 / 改类型与内容 / 取消激活 / 删除
        let rule = store.rename_variant("r1", &v1, "成功 v2").await.unwrap();
        assert_eq!(rule.response_variants[0].name, "成功 v2");
        let rule = store
            .update_variant(
                "r1",
                &v1,
                ResponseKind::Script,
                "return { status: 200, body: 'ok' };",
            )
            .await
            .unwrap();
        assert_eq!(rule.response_variants[0].kind, ResponseKind::Script);
        assert_eq!(
            rule.response_variants[0].content,
            "return { status: 200, body: 'ok' };"
        );
        let rule = store.set_selected_variant("r1", None).await.unwrap();
        assert_eq!(
            rule.effective_response(),
            (ResponseKind::Static, "{}".into()),
            "取消激活回退到规则本身的默认响应"
        );
        let rule = store.delete_variant("r1", &v2).await.unwrap();
        assert_eq!(rule.response_variants.len(), 1);
        // 删激活项 → 回退到规则本身
        store.set_selected_variant("r1", Some(&v1)).await.unwrap();
        let rule = store.delete_variant("r1", &v1).await.unwrap();
        assert!(rule.response_variants.is_empty());
        assert_eq!(rule.selected_variant_id, None);

        // 变体操作要求规则存在
        assert!(matches!(
            store
                .add_variant("missing", "x", ResponseKind::Static, "{}", false)
                .await,
            Err(MockproxyrsError::NotFound(_))
        ));
        // 选中不属于该规则的变体 → NotFound
        store
            .save_rule(sample_rule("r2", SERVICE_ID, "/api/z"))
            .await
            .unwrap();
        store
            .add_variant("r2", "other", ResponseKind::Static, "{}", false)
            .await
            .unwrap();
        let other = repo
            .get_rule("r2")
            .await
            .unwrap()
            .unwrap()
            .response_variants[0]
            .id
            .clone();
        assert!(
            store
                .set_selected_variant("r1", Some(&other))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_save_rule_for_stopped_service_only_persists() {
        let (store, repo, _, events) = setup().await;
        // svc-2 没有运行中的服务：只落库，不报错，仍然推送事件
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
        assert_eq!(take_events(&events).len(), 1);
    }
}
