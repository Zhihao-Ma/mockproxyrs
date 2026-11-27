//! 数据仓库模块
//!
//! 提供数据持久化接口和 SQLite 实现。
//!
//! # 主要组件
//!
//! - [`MockRepository`] - 数据仓库 trait，定义服务与规则的 CRUD 接口
//! - [`SqliteRepository`] - SQLite 实现，支持数据持久化

mod migrations;
mod sqlite;

use async_trait::async_trait;
pub use sqlite::SqliteRepository;

use crate::domain::{MockResponseVariant, MockRule, MockService, ResponseKind};
use crate::error::Result;

/// Mock 数据仓库 trait
///
/// 定义服务与规则的持久化操作接口。所有方法均为异步，支持非阻塞 IO。
///
/// 响应变体存在独立的表中，对变体的每次增删改都是行级写入，不会触碰所属规则的其它字段；
/// 读取规则时再把变体组装回 [`MockRule::response_variants`]。
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
    ///
    /// 只写入规则自身的字段；响应变体不由此处维护，请用下方的变体方法。
    async fn save_rule(&self, rule: &MockRule) -> Result<()>;

    /// 删除规则（级联删除其响应变体）
    async fn delete_rule(&self, id: &str) -> Result<()>;

    /// 删除服务的所有规则
    async fn delete_rules_by_service(&self, service_id: &str) -> Result<()>;

    /// 新增响应变体，追加到所属规则响应集的末尾
    async fn add_variant(&self, variant: &MockResponseVariant) -> Result<()>;

    /// 重命名响应变体
    ///
    /// 变体不存在时返回 [`crate::error::MockproxyrsError::NotFound`]。
    async fn rename_variant(&self, id: &str, name: &str) -> Result<()>;

    /// 更新响应变体的类型与内容（单一内容槽：切类型的同时内容被替换）
    ///
    /// 变体不存在时返回 [`crate::error::MockproxyrsError::NotFound`]。
    async fn update_variant(&self, id: &str, kind: ResponseKind, content: &str) -> Result<()>;

    /// 删除响应变体
    ///
    /// 在同一事务内完成：若被删的是所属规则当前激活的变体，则把激活项清空（回退到规则本身的
    /// mock_response）。变体不存在时返回 [`crate::error::MockproxyrsError::NotFound`]。
    async fn delete_variant(&self, id: &str) -> Result<()>;

    /// 设置规则当前激活的响应变体；`None` 表示不激活任何变体
    ///
    /// 规则或变体不存在、或变体不属于该规则时返回 [`crate::error::MockproxyrsError::NotFound`]。
    async fn set_selected_variant(&self, rule_id: &str, variant_id: Option<&str>) -> Result<()>;
}
