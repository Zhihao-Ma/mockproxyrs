//! 数据仓库模块
//!
//! 提供数据持久化接口和 SQLite 实现。
//!
//! # 主要组件
//!
//! - [`MockRepository`] - 数据仓库 trait，定义服务与规则的 CRUD 接口
//! - [`SqliteRepository`] - SQLite 实现，支持数据持久化

mod sqlite;

use async_trait::async_trait;
pub use sqlite::SqliteRepository;

use crate::domain::{MockRule, MockService};
use crate::error::Result;

/// Mock 数据仓库 trait
///
/// 定义服务与规则的持久化操作接口。
/// 所有方法均为异步，支持非阻塞 IO。
#[async_trait]
pub trait MockRepository: Send + Sync {
    /// 获取所有服务
    async fn list_services(&self) -> Result<Vec<MockService>>;

    /// 根据 ID 获取服务
    async fn get_service(&self, id: &str) -> Result<Option<MockService>>;

    /// 保存服务（新增或更新）
    async fn save_service(&self, service: &MockService) -> Result<()>;

    /// 删除服务
    async fn delete_service(&self, id: &str) -> Result<()>;

    /// 获取服务的所有规则
    async fn list_rules(&self, service_id: &str) -> Result<Vec<MockRule>>;

    /// 根据 ID 获取规则
    async fn get_rule(&self, id: &str) -> Result<Option<MockRule>>;

    /// 保存规则（新增或更新）
    async fn save_rule(&self, rule: &MockRule) -> Result<()>;

    /// 删除规则
    async fn delete_rule(&self, id: &str) -> Result<()>;

    /// 删除服务的所有规则
    async fn delete_rules_by_service(&self, service_id: &str) -> Result<()>;
}
