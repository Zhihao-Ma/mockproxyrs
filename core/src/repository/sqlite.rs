use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};
use tokio::sync::RwLock;

use super::MockRepository;
use super::migrations::MIGRATIONS;
use crate::domain::{Method, MockResponseVariant, MockRule, MockService, ResponseKind};
use crate::error::{MockproxyrsError, Result};

/// SQLite 数据仓库实现
pub struct SqliteRepository {
    pool: Pool<SqliteConnectionManager>,
    /// 写锁，确保写操作串行
    write_lock: Arc<RwLock<()>>,
}

impl SqliteRepository {
    /// 创建新的 SQLite 仓库
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let manager = SqliteConnectionManager::file(path);
        let pool = Pool::builder()
            .build(manager)
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        // 初始化表结构
        let mut conn = pool
            .get()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        MIGRATIONS
            .to_latest(&mut conn)
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(Self {
            pool,
            write_lock: Arc::new(RwLock::new(())),
        })
    }

    /// 创建内存数据库（用于测试）
    pub fn in_memory() -> Result<Self> {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder()
            .build(manager)
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        let mut conn = pool
            .get()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        MIGRATIONS
            .to_latest(&mut conn)
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(Self {
            pool,
            write_lock: Arc::new(RwLock::new(())),
        })
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool
            .get()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))
    }

    /// 将 Method 枚举转换为字符串
    fn method_to_string(method: &Method) -> &'static str {
        match method {
            Method::All => "ALL",
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        }
    }

    /// 从字符串解析 Method 枚举
    fn parse_method(s: &str) -> Method {
        match s.to_uppercase().as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            _ => Method::All,
        }
    }
}

#[async_trait]
impl MockRepository for SqliteRepository {
    async fn list_services(&self) -> Result<Vec<MockService>> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare("SELECT id, name, listen_addr, target_url FROM mock_service")
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        let services = stmt
            .query_map([], |row| {
                Ok(MockService {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    listen_addr: row.get(2)?,
                    target_url: row.get(3)?,
                })
            })
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(services)
    }

    async fn get_service(&self, id: &str) -> Result<Option<MockService>> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare("SELECT id, name, listen_addr, target_url FROM mock_service WHERE id = ?")
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        let result = stmt
            .query_row(params![id], |row| {
                Ok(MockService {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    listen_addr: row.get(2)?,
                    target_url: row.get(3)?,
                })
            })
            .optional()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(result)
    }

    async fn save_service(&self, service: &MockService) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        // 使用 UPSERT 语法
        conn.execute(
            r#"INSERT INTO mock_service (id, name, listen_addr, target_url)
               VALUES (?1, ?2, ?3, ?4)
               ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   listen_addr = excluded.listen_addr,
                   target_url = excluded.target_url"#,
            params![
                service.id,
                service.name,
                service.listen_addr,
                service.target_url
            ],
        )
        .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(())
    }

    async fn delete_service(&self, id: &str) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        conn.execute("DELETE FROM mock_service WHERE id = ?", params![id])
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_rules(&self, service_id: &str) -> Result<Vec<MockRule>> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response, delay_ms, use_script, selected_variant_id
                 FROM mock_rule WHERE service_id = ?
                 ORDER BY created_at DESC, rowid DESC",
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        let rules = stmt
            .query_map(params![service_id], Self::row_to_rule)
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        drop(stmt);

        // 一次查询取回该服务下全部变体再按规则分组，避免逐条规则 N+1 查询
        let mut variants_by_rule: HashMap<String, Vec<MockResponseVariant>> = HashMap::new();
        for v in self.list_variants_by_service(&conn, service_id)? {
            variants_by_rule
                .entry(v.rule_id.clone())
                .or_default()
                .push(v);
        }

        Ok(rules
            .into_iter()
            .map(|mut rule| {
                rule.response_variants = variants_by_rule.remove(&rule.id).unwrap_or_default();
                rule
            })
            .collect())
    }

    async fn get_rule(&self, id: &str) -> Result<Option<MockRule>> {
        let conn = self.get_conn()?;
        let result = conn
            .query_row(
                "SELECT id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response, delay_ms, use_script, selected_variant_id
                 FROM mock_rule WHERE id = ?",
                params![id],
                Self::row_to_rule,
            )
            .optional()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(result.map(|mut rule| {
            rule.response_variants = self
                .list_variants_by_rule(&conn, &rule.id)
                .unwrap_or_default();
            rule
        }))
    }

    /// 只写规则自身的列；响应变体由 [`MockRepository::add_variant`] 等方法按行维护，
    /// 当前激活项由 [`MockRepository::set_selected_variant`] 单独维护（更新已有规则时不覆盖，
    /// 避免用旧快照保存规则时把别处刚切换的激活项冲掉；新插入的行该列恒为 NULL）。
    async fn save_rule(&self, rule: &MockRule) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        let method = Self::method_to_string(&rule.method);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        conn.execute(
            r#"INSERT INTO mock_rule (id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response, delay_ms, use_script, selected_variant_id, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
               ON CONFLICT(id) DO UPDATE SET
                   url_pattern = excluded.url_pattern,
                   is_regex = excluded.is_regex,
                   method = excluded.method,
                   enabled = excluded.enabled,
                   forward_and_record = excluded.forward_and_record,
                   mock_response = excluded.mock_response,
                   delay_ms = excluded.delay_ms,
                   use_script = excluded.use_script"#,
            params![
                rule.id,
                rule.service_id,
                rule.url_pattern,
                rule.is_regex as i32,
                method,
                rule.enabled as i32,
                rule.forward_and_record as i32,
                rule.mock_response,
                rule.delay_ms.map(|v| v as i64),
                rule.use_script as i32,
                rule.selected_variant_id.as_deref(),
                now_ms,
            ],
        )
        .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(())
    }

    async fn delete_rule(&self, id: &str) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        conn.execute("DELETE FROM mock_rule WHERE id = ?", params![id])
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(())
    }

    async fn delete_rules_by_service(&self, service_id: &str) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        conn.execute(
            "DELETE FROM mock_rule WHERE service_id = ?",
            params![service_id],
        )
        .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(())
    }

    async fn add_variant(&self, variant: &MockResponseVariant) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        // 追加到末尾：position 取当前最大值 + 1，删除后不重排（留空隙不影响顺序）
        conn.execute(
            "INSERT INTO mock_response_variant (id, rule_id, name, kind, content, position)
             VALUES (?1, ?2, ?3, ?4, ?5,
                     (SELECT COALESCE(MAX(position), -1) + 1 FROM mock_response_variant WHERE rule_id = ?2))",
            params![
                variant.id,
                variant.rule_id,
                variant.name,
                variant.kind.to_string(),
                variant.content
            ],
        )
        .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(())
    }

    async fn rename_variant(&self, id: &str, name: &str) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        let changed = conn
            .execute(
                "UPDATE mock_response_variant SET name = ?1 WHERE id = ?2",
                params![name, id],
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        Self::require_affected(changed, id)
    }

    async fn update_variant(&self, id: &str, kind: ResponseKind, content: &str) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        let changed = conn
            .execute(
                "UPDATE mock_response_variant SET kind = ?1, content = ?2 WHERE id = ?3",
                params![kind.to_string(), content, id],
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        Self::require_affected(changed, id)
    }

    async fn delete_variant(&self, id: &str) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let mut conn = self.get_conn()?;

        let tx = conn
            .transaction()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        // 删激活中的变体 → 回退到规则本身的 mock_response（置 NULL）
        tx.execute(
            "UPDATE mock_rule SET selected_variant_id = NULL WHERE selected_variant_id = ?1",
            params![id],
        )
        .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        let changed = tx
            .execute(
                "DELETE FROM mock_response_variant WHERE id = ?1",
                params![id],
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        Self::require_affected(changed, id)?;

        tx.commit()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))
    }

    async fn set_selected_variant(&self, rule_id: &str, variant_id: Option<&str>) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        // 变体必须属于该规则；置 NULL 时跳过校验
        if let Some(vid) = variant_id {
            let belongs: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM mock_response_variant WHERE id = ?1 AND rule_id = ?2)",
                    params![vid, rule_id],
                    |row| row.get(0),
                )
                .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
            if !belongs {
                return Err(MockproxyrsError::NotFound(format!(
                    "响应变体不存在或不属于该规则: {}",
                    vid
                )));
            }
        }

        let changed = conn
            .execute(
                "UPDATE mock_rule SET selected_variant_id = ?1 WHERE id = ?2",
                params![variant_id, rule_id],
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        Self::require_affected(changed, rule_id)
    }
}

/// 行级写入的辅助函数与行映射
impl SqliteRepository {
    /// 把 `mock_rule` 的一行映射为 [`MockRule`]（不含响应变体，由调用方补齐）
    ///
    /// 列顺序须与 list_rules / get_rule 的 SELECT 一致：
    /// 0 id, 1 service_id, 2 url_pattern, 3 is_regex, 4 method, 5 enabled, 6 forward_and_record,
    /// 7 mock_response, 8 delay_ms, 9 use_script, 10 selected_variant_id
    fn row_to_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<MockRule> {
        let method: String = row.get(4)?;
        let is_regex: i32 = row.get(3)?;
        let enabled: i32 = row.get(5)?;
        let forward_and_record: i32 = row.get(6)?;
        let use_script: i32 = row.get(9)?;
        Ok(MockRule {
            id: row.get(0)?,
            service_id: row.get(1)?,
            url_pattern: row.get(2)?,
            is_regex: is_regex != 0,
            method: Self::parse_method(&method),
            enabled: enabled != 0,
            forward_and_record: forward_and_record != 0,
            mock_response: row.get(7)?,
            delay_ms: row.get(8)?,
            use_script: use_script != 0,
            response_variants: Vec::new(),
            selected_variant_id: row.get(10)?,
        })
    }

    /// 变体行映射（列顺序：0 id, 1 rule_id, 2 name, 3 kind, 4 content）
    fn row_to_variant(row: &rusqlite::Row<'_>) -> rusqlite::Result<MockResponseVariant> {
        let kind: String = row.get(3)?;
        Ok(MockResponseVariant {
            id: row.get(0)?,
            rule_id: row.get(1)?,
            name: row.get(2)?,
            kind: parse_kind(&kind),
            content: row.get(4)?,
        })
    }

    /// 某规则的全部变体，按 position 升序
    fn list_variants_by_rule(
        &self,
        conn: &rusqlite::Connection,
        rule_id: &str,
    ) -> Result<Vec<MockResponseVariant>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, rule_id, name, kind, content FROM mock_response_variant
                 WHERE rule_id = ?1 ORDER BY position ASC, rowid ASC",
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        stmt.query_map(params![rule_id], Self::row_to_variant)
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))
    }

    /// 某服务下全部规则的变体，按 position 升序
    fn list_variants_by_service(
        &self,
        conn: &rusqlite::Connection,
        service_id: &str,
    ) -> Result<Vec<MockResponseVariant>> {
        let mut stmt = conn
            .prepare(
                "SELECT v.id, v.rule_id, v.name, v.kind, v.content
                 FROM mock_response_variant v
                 JOIN mock_rule r ON r.id = v.rule_id
                 WHERE r.service_id = ?1
                 ORDER BY v.position ASC, v.rowid ASC",
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        stmt.query_map(params![service_id], Self::row_to_variant)
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))
    }

    /// 行级写入影响行数为 0 时视为目标不存在
    fn require_affected(changed: usize, id: &str) -> Result<()> {
        if changed == 0 {
            return Err(MockproxyrsError::NotFound(format!(
                "响应变体不存在: {}",
                id
            )));
        }
        Ok(())
    }
}

/// 解析变体类型；未知值回退为静态，保证旧库或异常数据不会让读取失败
fn parse_kind(s: &str) -> ResponseKind {
    match s {
        "script" => ResponseKind::Script,
        _ => ResponseKind::Static,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MockResponseVariant;

    #[tokio::test]
    async fn test_service_crud() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "test-id".to_string(),
            "Test Service".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );

        // Create
        repo.save_service(&service).await.unwrap();

        // Read
        let found = repo.get_service("test-id").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap(), service);

        // List
        let services = repo.list_services().await.unwrap();
        assert_eq!(services.len(), 1);

        // Delete
        repo.delete_service("test-id").await.unwrap();
        let found = repo.get_service("test-id").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_rule_crud() {
        let repo = SqliteRepository::in_memory().unwrap();

        // 先创建服务
        let service = MockService::new(
            "service-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        let rule = MockRule::new(
            "rule-1".to_string(),
            "service-1".to_string(),
            "/api/test".to_string(),
            false,
            Method::Get,
            true,
            false,
            r#"{"code": 200}"#.to_string(),
            None,
            false,
        );

        // Create
        repo.save_rule(&rule).await.unwrap();

        // Read
        let found = repo.get_rule("rule-1").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap(), rule);

        // List by service
        let rules = repo.list_rules("service-1").await.unwrap();
        assert_eq!(rules.len(), 1);

        // Delete
        repo.delete_rule("rule-1").await.unwrap();
        let found = repo.get_rule("rule-1").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_list_rules_order_newest_first() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-order".to_string(),
            "Order".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        let mk_rule = |id: &str, url: &str| {
            MockRule::new(
                id.to_string(),
                "svc-order".to_string(),
                url.to_string(),
                false,
                Method::Get,
                true,
                false,
                r#"{"code": 200}"#.to_string(),
                None,
                false,
            )
        };

        repo.save_rule(&mk_rule("rule-old", "/api/old"))
            .await
            .unwrap();
        repo.save_rule(&mk_rule("rule-new", "/api/new"))
            .await
            .unwrap();

        let rules = repo.list_rules("svc-order").await.unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].id, "rule-new");
        assert_eq!(rules[1].id, "rule-old");
    }

    #[tokio::test]
    async fn test_rule_update_keeps_original_order() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-order2".to_string(),
            "Order2".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        let mk_rule = |id: &str, url: &str| {
            MockRule::new(
                id.to_string(),
                "svc-order2".to_string(),
                url.to_string(),
                false,
                Method::Get,
                true,
                false,
                r#"{"code": 200}"#.to_string(),
                None,
                false,
            )
        };

        repo.save_rule(&mk_rule("rule-a", "/api/a")).await.unwrap();
        repo.save_rule(&mk_rule("rule-b", "/api/b")).await.unwrap();

        repo.save_rule(&mk_rule("rule-a", "/api/a2")).await.unwrap();

        let rules = repo.list_rules("svc-order2").await.unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].id, "rule-b");
        assert_eq!(rules[1].id, "rule-a");
    }

    #[tokio::test]
    async fn test_service_upsert() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Original".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        // Update with same id
        let updated = MockService::new(
            "svc-1".to_string(),
            "Updated".to_string(),
            "127.0.0.1:9090".to_string(),
            "https://updated.com".to_string(),
        );
        repo.save_service(&updated).await.unwrap();

        // Should have only one service
        let services = repo.list_services().await.unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "Updated");
        assert_eq!(services[0].listen_addr, "127.0.0.1:9090");
    }

    #[tokio::test]
    async fn test_rule_upsert() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        let rule = MockRule::new(
            "rule-1".to_string(),
            "svc-1".to_string(),
            "/api/test".to_string(),
            false,
            Method::Get,
            true,
            false,
            r#"{"code": 200}"#.to_string(),
            None,
            false,
        );
        repo.save_rule(&rule).await.unwrap();

        // Update with same id
        let updated = MockRule::new(
            "rule-1".to_string(),
            "svc-1".to_string(),
            "/api/updated".to_string(),
            true,
            Method::Post,
            false,
            true,
            r#"{"code": 201}"#.to_string(),
            None,
            false,
        );
        repo.save_rule(&updated).await.unwrap();

        // Should have only one rule
        let rules = repo.list_rules("svc-1").await.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].url_pattern, "/api/updated");
        assert_eq!(rules[0].method, Method::Post);
        assert!(!rules[0].enabled);
        assert!(rules[0].forward_and_record);
    }

    #[tokio::test]
    async fn test_list_empty_services() {
        let repo = SqliteRepository::in_memory().unwrap();
        let services = repo.list_services().await.unwrap();
        assert!(services.is_empty());
    }

    #[tokio::test]
    async fn test_list_empty_rules() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        let rules = repo.list_rules("svc-1").await.unwrap();
        assert!(rules.is_empty());
    }

    #[tokio::test]
    async fn test_get_nonexistent_service() {
        let repo = SqliteRepository::in_memory().unwrap();
        let found = repo.get_service("nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_get_nonexistent_rule() {
        let repo = SqliteRepository::in_memory().unwrap();
        let found = repo.get_rule("nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_service() {
        let repo = SqliteRepository::in_memory().unwrap();
        // Should not error
        repo.delete_service("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_nonexistent_rule() {
        let repo = SqliteRepository::in_memory().unwrap();
        // Should not error
        repo.delete_rule("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_rules_by_service() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        // Create multiple rules
        for i in 0..3 {
            let rule = MockRule::new(
                format!("rule-{}", i),
                "svc-1".to_string(),
                format!("/api/{}", i),
                false,
                Method::Get,
                true,
                false,
                "{}".to_string(),
                None,
                false,
            );
            repo.save_rule(&rule).await.unwrap();
        }

        let rules = repo.list_rules("svc-1").await.unwrap();
        assert_eq!(rules.len(), 3);

        // Delete all rules
        repo.delete_rules_by_service("svc-1").await.unwrap();
        let rules = repo.list_rules("svc-1").await.unwrap();
        assert!(rules.is_empty());
    }

    #[tokio::test]
    async fn test_rule_all_methods() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        // Test all method types
        for method in [
            Method::All,
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Delete,
        ] {
            let rule = MockRule::new(
                format!("rule-{:?}", method),
                "svc-1".to_string(),
                "/api/test".to_string(),
                false,
                method,
                true,
                false,
                "{}".to_string(),
                None,
                false,
            );
            repo.save_rule(&rule).await.unwrap();

            let found = repo.get_rule(&format!("rule-{:?}", method)).await.unwrap();
            assert!(found.is_some());
            assert_eq!(found.unwrap().method, method);
        }
    }

    #[tokio::test]
    async fn test_rule_enabled_disabled() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        let rule_enabled = MockRule::new(
            "rule-enabled".to_string(),
            "svc-1".to_string(),
            "/api/test".to_string(),
            false,
            Method::Get,
            true,
            false,
            "{}".to_string(),
            None,
            false,
        );
        repo.save_rule(&rule_enabled).await.unwrap();

        let rule_disabled = MockRule::new(
            "rule-disabled".to_string(),
            "svc-1".to_string(),
            "/api/test".to_string(),
            false,
            Method::Get,
            false,
            false,
            "{}".to_string(),
            None,
            false,
        );
        repo.save_rule(&rule_disabled).await.unwrap();

        let found = repo.get_rule("rule-enabled").await.unwrap().unwrap();
        assert!(found.enabled);

        let found = repo.get_rule("rule-disabled").await.unwrap().unwrap();
        assert!(!found.enabled);
    }

    #[tokio::test]
    async fn test_rule_forward_and_record() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        let rule = MockRule::new(
            "rule-1".to_string(),
            "svc-1".to_string(),
            "/api/test".to_string(),
            false,
            Method::Get,
            true,
            true,
            "{}".to_string(),
            None,
            false,
        );
        repo.save_rule(&rule).await.unwrap();

        let found = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert!(found.forward_and_record);
    }

    #[tokio::test]
    async fn test_multiple_services() {
        let repo = SqliteRepository::in_memory().unwrap();

        for i in 0..5 {
            let service = MockService::new(
                format!("svc-{}", i),
                format!("Service {}", i),
                format!("127.0.0.1:{}", 8080 + i),
                format!("https://example{}.com", i),
            );
            repo.save_service(&service).await.unwrap();
        }

        let services = repo.list_services().await.unwrap();
        assert_eq!(services.len(), 5);
    }

    #[tokio::test]
    async fn test_multiple_rules_per_service() {
        let repo = SqliteRepository::in_memory().unwrap();

        let service = MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        );
        repo.save_service(&service).await.unwrap();

        for i in 0..10 {
            let rule = MockRule::new(
                format!("rule-{}", i),
                "svc-1".to_string(),
                format!("/api/{}", i),
                false,
                Method::Get,
                true,
                false,
                "{}".to_string(),
                None,
                false,
            );
            repo.save_rule(&rule).await.unwrap();
        }

        let rules = repo.list_rules("svc-1").await.unwrap();
        assert_eq!(rules.len(), 10);
    }

    /// 从最原始的库结构（001 之前）升级到最新，验证全链路迁移后的最终状态
    #[tokio::test]
    async fn test_migration_from_legacy_database_to_latest() {
        let path =
            std::env::temp_dir().join(format!("mockproxyrs-migration-{}.db", uuid::Uuid::new_v4()));

        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE mock_service (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    listen_addr TEXT NOT NULL,
                    target_url TEXT NOT NULL
                );

                CREATE TABLE mock_rule (
                    id TEXT PRIMARY KEY,
                    service_id TEXT NOT NULL,
                    url_pattern TEXT NOT NULL,
                    is_regex INTEGER NOT NULL DEFAULT 0,
                    method TEXT NOT NULL DEFAULT 'ALL',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    forward_and_record INTEGER NOT NULL DEFAULT 0,
                    mock_response TEXT,
                    FOREIGN KEY (service_id) REFERENCES mock_service(id) ON DELETE CASCADE
                );

                INSERT INTO mock_service (id, name, listen_addr, target_url)
                VALUES ('svc-1', 'Test', '127.0.0.1:8080', 'https://example.com');

                INSERT INTO mock_rule (id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response)
                VALUES ('rule-1', 'svc-1', '/api/test', 0, 'GET', 1, 0, '{"code":200}');
                "#,
            )
            .unwrap();
        }

        let repo = SqliteRepository::new(&path).unwrap();
        let rule = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(rule.delay_ms, None);
        assert!(!rule.use_script, "存量规则默认响应为静态");
        // 响应集列在迁移 005 中加入、变体在 006 中分表，存量规则应得到安全的缺省值
        assert!(
            rule.response_variants.is_empty(),
            "存量规则迁移后无响应变体"
        );
        assert_eq!(rule.selected_variant_id, None);

        // 脚本列已在 007 并入 mock_response：默认响应改为脚本后，内容存在同一个列里
        let updated = MockRule::new(
            "rule-1".to_string(),
            "svc-1".to_string(),
            "/api/test".to_string(),
            false,
            Method::Get,
            true,
            false,
            "return { code: 0 };".to_string(),
            Some(250),
            true,
        );
        repo.save_rule(&updated).await.unwrap();
        let found = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(
            found.effective_response(),
            (ResponseKind::Script, "return { code: 0 };".into())
        );
        assert_eq!(found.delay_ms, Some(250));

        let _ = std::fs::remove_file(path);
    }

    /// 构造一条带两个变体（一静态一脚本）、选中 v2 的规则并写入仓库
    async fn seed_rule_with_variants(repo: &SqliteRepository) -> MockRule {
        repo.save_service(&MockService::new(
            "svc-1".to_string(),
            "Test".to_string(),
            "127.0.0.1:8080".to_string(),
            "https://example.com".to_string(),
        ))
        .await
        .unwrap();

        let rule = MockRule::new(
            "rule-1".to_string(),
            "svc-1".to_string(),
            "/api/test".to_string(),
            false,
            Method::Get,
            true,
            false,
            "{\"code\":200}".to_string(),
            None,
            false,
        );
        repo.save_rule(&rule).await.unwrap();

        repo.add_variant(&MockResponseVariant {
            id: "v1".to_string(),
            rule_id: "rule-1".to_string(),
            name: "成功".to_string(),
            kind: ResponseKind::Static,
            content: "{\"ok\":true}".to_string(),
        })
        .await
        .unwrap();
        repo.add_variant(&MockResponseVariant {
            id: "v2".to_string(),
            rule_id: "rule-1".to_string(),
            name: "失败".to_string(),
            kind: ResponseKind::Script,
            content: "return { status: 500, body: 'boom' };".to_string(),
        })
        .await
        .unwrap();
        repo.set_selected_variant("rule-1", Some("v2"))
            .await
            .unwrap();

        let mut rule = repo.get_rule("rule-1").await.unwrap().unwrap();
        rule.response_variants = vec![
            MockResponseVariant {
                id: "v1".to_string(),
                rule_id: "rule-1".to_string(),
                name: "成功".to_string(),
                kind: ResponseKind::Static,
                content: "{\"ok\":true}".to_string(),
            },
            MockResponseVariant {
                id: "v2".to_string(),
                rule_id: "rule-1".to_string(),
                name: "失败".to_string(),
                kind: ResponseKind::Script,
                content: "return { status: 500, body: 'boom' };".to_string(),
            },
        ];
        rule.selected_variant_id = Some("v2".to_string());
        rule
    }

    #[tokio::test]
    async fn test_rule_response_variants_roundtrip() {
        let repo = SqliteRepository::in_memory().unwrap();
        let expected = seed_rule_with_variants(&repo).await;

        // list_rules 与 get_rule 两条读取路径都应完整还原响应集（含顺序与类型）
        let listed = repo.list_rules("svc-1").await.unwrap();
        assert_eq!(listed, vec![expected.clone()]);
        let got = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(got, expected);
        assert_eq!(
            got.effective_response(),
            (
                ResponseKind::Script,
                "return { status: 500, body: 'boom' };".into()
            )
        );
    }

    /// 分表的核心保证：保存规则（只写规则自身的列）不会能掉已有的响应变体。
    /// 复现的场景：前端打开页面后，另一端通过 MCP 加了变体；前端用旧快照改 URL 并保存。
    #[tokio::test]
    async fn test_save_rule_preserves_variants_added_elsewhere() {
        let repo = SqliteRepository::in_memory().unwrap();
        seed_rule_with_variants(&repo).await;

        // 模拟前端持有的旧快照：没有任何变体、未选中
        let mut stale = repo.get_rule("rule-1").await.unwrap().unwrap();
        stale.response_variants.clear();
        stale.selected_variant_id = None;
        stale.url_pattern = "/api/renamed".to_string();
        repo.save_rule(&stale).await.unwrap();

        let got = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(got.url_pattern, "/api/renamed", "规则字段正常更新");
        assert_eq!(got.response_variants.len(), 2, "变体不受保存规则影响");
        assert_eq!(got.response_variants[0].id, "v1", "顺序保持");
        assert_eq!(got.response_variants[1].id, "v2");
        assert_eq!(
            got.selected_variant_id.as_deref(),
            Some("v2"),
            "激活项不受影响"
        );
    }

    #[tokio::test]
    async fn test_variant_row_operations() {
        let repo = SqliteRepository::in_memory().unwrap();
        seed_rule_with_variants(&repo).await;

        // 重命名与更新（类型 + 内容）是行级写入
        repo.rename_variant("v1", "成功 v2").await.unwrap();
        repo.update_variant(
            "v1",
            ResponseKind::Script,
            "return { status: 200, body: 'ok' };",
        )
        .await
        .unwrap();
        let got = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(got.response_variants[0].name, "成功 v2");
        assert_eq!(
            got.response_variants[0].kind,
            ResponseKind::Script,
            "类型切换为脚本"
        );
        assert_eq!(
            got.response_variants[0].content,
            "return { status: 200, body: 'ok' };"
        );
        assert_eq!(got.response_variants[1].name, "失败", "其它变体不受影响");

        // 切回静态：内容整体替换
        repo.update_variant("v1", ResponseKind::Static, "{\"ok\":1}")
            .await
            .unwrap();
        let got = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(got.response_variants[0].kind, ResponseKind::Static);
        assert_eq!(got.response_variants[0].content, "{\"ok\":1}");

        // 删除非激活变体：激活项不变
        repo.delete_variant("v1").await.unwrap();
        let got = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(got.response_variants.len(), 1);
        assert_eq!(got.selected_variant_id.as_deref(), Some("v2"));

        // 删除激活变体：回退到规则本身（NULL）
        repo.delete_variant("v2").await.unwrap();
        let got = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert!(got.response_variants.is_empty());
        assert_eq!(
            got.selected_variant_id, None,
            "删除激活项后回退到规则默认响应"
        );

        // 不存在的变体 → NotFound
        assert!(repo.delete_variant("nope").await.is_err());
        assert!(repo.rename_variant("nope", "x").await.is_err());
        assert!(
            repo.update_variant("nope", ResponseKind::Static, "x")
                .await
                .is_err()
        );
    }

    /// 007 迁移：script 列并入 mock_response，变体表加 kind、内容列改名
    #[tokio::test]
    async fn test_migration_unifies_response_kind() {
        let path = std::env::temp_dir().join(format!(
            "mockproxyrs-migration-007-{}.db",
            uuid::Uuid::new_v4()
        ));

        // 构造 006 之后、007 之前的库结构（含一条脚本规则、一条脚本为空的脚本规则、一条静态规则）
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE mock_service (id TEXT PRIMARY KEY, name TEXT NOT NULL, listen_addr TEXT NOT NULL, target_url TEXT NOT NULL);
                CREATE TABLE mock_rule (
                    id TEXT PRIMARY KEY, service_id TEXT NOT NULL, url_pattern TEXT NOT NULL,
                    is_regex INTEGER NOT NULL DEFAULT 0, method TEXT NOT NULL DEFAULT 'ALL',
                    enabled INTEGER NOT NULL DEFAULT 1, forward_and_record INTEGER NOT NULL DEFAULT 0,
                    mock_response TEXT NOT NULL DEFAULT '', script TEXT, delay_ms INTEGER,
                    use_script INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL DEFAULT 0,
                    selected_variant_id TEXT,
                    FOREIGN KEY (service_id) REFERENCES mock_service(id) ON DELETE CASCADE
                );
                CREATE TABLE mock_response_variant (
                    id TEXT PRIMARY KEY, rule_id TEXT NOT NULL REFERENCES mock_rule(id) ON DELETE CASCADE,
                    name TEXT NOT NULL, mock_response TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO mock_service VALUES ('svc-1', 'Test', '127.0.0.1:8080', 'https://example.com');
                INSERT INTO mock_rule (id, service_id, url_pattern, mock_response, script, use_script) VALUES
                    ('r-script', 'svc-1', '/a', '{"unused":true}', 'return { status: 201, body: "created" };', 1),
                    ('r-empty',  'svc-1', '/b', '{"keep":true}',    '   ',                           1),
                    ('r-static', 'svc-1', '/c', '{"static":true}',   NULL,                           0);
                INSERT INTO mock_response_variant (id, rule_id, name, mock_response, position) VALUES
                    ('v1', 'r-static', 'old', '{"v":1}', 0);
                PRAGMA user_version = 6;
                "#,
            )
            .unwrap();
        }

        let repo = SqliteRepository::new(&path).unwrap();
        let rules = repo.list_rules("svc-1").await.unwrap();
        let by_id = |id: &str| rules.iter().find(|r| r.id == id).unwrap();

        // 有脚本内容：脚本成为默认响应内容，未生效的 JSON 丢弃
        assert_eq!(
            by_id("r-script").effective_response(),
            (
                ResponseKind::Script,
                "return { status: 201, body: \"created\" };".into()
            )
        );
        // 脚本模式但脚本为空：退回静态，原 JSON 保留
        assert_eq!(
            by_id("r-empty").effective_response(),
            (ResponseKind::Static, "{\"keep\":true}".into())
        );
        // 静态规则不受影响
        assert_eq!(
            by_id("r-static").effective_response(),
            (ResponseKind::Static, "{\"static\":true}".into())
        );
        // 存量变体：kind 默认静态，内容列改名后仍可读
        let v = &by_id("r-static").response_variants[0];
        assert_eq!(v.kind, ResponseKind::Static);
        assert_eq!(v.content, "{\"v\":1}");

        // script 列已删、变体表列已改名
        let conn = repo.get_conn().unwrap();
        let rule_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('mock_rule')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(!rule_cols.iter().any(|c| c == "script"));
        let var_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('mock_response_variant')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(var_cols.iter().any(|c| c == "kind"));
        assert!(var_cols.iter().any(|c| c == "content"));
        assert!(!var_cols.iter().any(|c| c == "mock_response"));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_set_selected_variant_validates_membership() {
        let repo = SqliteRepository::in_memory().unwrap();
        seed_rule_with_variants(&repo).await;

        // 另一条规则上的变体不能被选中
        let other = MockRule::new(
            "rule-2".to_string(),
            "svc-1".to_string(),
            "/api/other".to_string(),
            false,
            Method::Get,
            true,
            false,
            "{}".to_string(),
            None,
            false,
        );
        repo.save_rule(&other).await.unwrap();
        assert!(
            repo.set_selected_variant("rule-2", Some("v1"))
                .await
                .is_err()
        );

        // 取消选中（None）总是允许
        repo.set_selected_variant("rule-1", None).await.unwrap();
        let got = repo.get_rule("rule-1").await.unwrap().unwrap();
        assert_eq!(got.selected_variant_id, None);
        assert_eq!(
            got.effective_response(),
            (ResponseKind::Static, "{\"code\":200}".into()),
            "回退到规则本身"
        );

        // 规则不存在 → NotFound
        assert!(repo.set_selected_variant("missing", None).await.is_err());
    }

    /// 删除规则级联删除变体
    #[tokio::test]
    async fn test_delete_rule_cascades_variants() {
        let repo = SqliteRepository::in_memory().unwrap();
        seed_rule_with_variants(&repo).await;
        repo.delete_rule("rule-1").await.unwrap();

        let conn = repo.get_conn().unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM mock_response_variant", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }

    /// 006 迁移：从带 JSON 列的旧库升级，旧变体丢弃、selected 清空、规则字段完好、JSON 列被删
    #[tokio::test]
    async fn test_migration_extracts_response_variants() {
        let path = std::env::temp_dir().join(format!(
            "mockproxyrs-migration-006-{}.db",
            uuid::Uuid::new_v4()
        ));

        // 构造 005 之后、006 之前的库结构
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE mock_service (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    listen_addr TEXT NOT NULL,
                    target_url TEXT NOT NULL
                );
                CREATE TABLE mock_rule (
                    id TEXT PRIMARY KEY,
                    service_id TEXT NOT NULL,
                    url_pattern TEXT NOT NULL,
                    is_regex INTEGER NOT NULL DEFAULT 0,
                    method TEXT NOT NULL DEFAULT 'ALL',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    forward_and_record INTEGER NOT NULL DEFAULT 0,
                    mock_response TEXT NOT NULL DEFAULT '',
                    script TEXT,
                    delay_ms INTEGER,
                    use_script INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    response_variants TEXT NOT NULL DEFAULT '[]',
                    selected_variant_id TEXT,
                    FOREIGN KEY (service_id) REFERENCES mock_service(id) ON DELETE CASCADE
                );
                INSERT INTO mock_service (id, name, listen_addr, target_url)
                VALUES ('svc-1', 'Test', '127.0.0.1:8080', 'https://example.com');
                INSERT INTO mock_rule (id, service_id, url_pattern, method, enabled, forward_and_record, mock_response, response_variants, selected_variant_id)
                VALUES (
                    'rule-1', 'svc-1', '/api/test', 'GET', 1, 0, '{"code":200}',
                    '[{"id":"v1","ruleId":"rule-1","name":"old","mockResponse":"{}"}]',
                    'v1'
                );
                PRAGMA user_version = 5;
                "#,
            )
            .unwrap();
        }

        let repo = SqliteRepository::new(&path).unwrap();
        let rules = repo.list_rules("svc-1").await.unwrap();
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert_eq!(rule.url_pattern, "/api/test", "规则字段完好");
        assert_eq!(rule.mock_response, "{\"code\":200}");
        assert!(rule.response_variants.is_empty(), "旧变体已丢弃");
        assert_eq!(rule.selected_variant_id, None, "selected 已清空");

        // JSON 列已被删除
        let conn = repo.get_conn().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('mock_rule')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(!cols.iter().any(|c| c == "response_variants"));
        assert!(cols.iter().any(|c| c == "selected_variant_id"));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_migration_backfills_created_at_for_legacy_order() {
        let path =
            std::env::temp_dir().join(format!("mockproxyrs-migration-{}.db", uuid::Uuid::new_v4()));

        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE mock_service (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    listen_addr TEXT NOT NULL,
                    target_url TEXT NOT NULL
                );

                CREATE TABLE mock_rule (
                    id TEXT PRIMARY KEY,
                    service_id TEXT NOT NULL,
                    url_pattern TEXT NOT NULL,
                    is_regex INTEGER NOT NULL DEFAULT 0,
                    method TEXT NOT NULL DEFAULT 'ALL',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    forward_and_record INTEGER NOT NULL DEFAULT 0,
                    mock_response TEXT,
                    FOREIGN KEY (service_id) REFERENCES mock_service(id) ON DELETE CASCADE
                );

                INSERT INTO mock_service (id, name, listen_addr, target_url)
                VALUES ('svc-1', 'Test', '127.0.0.1:8080', 'https://example.com');

                INSERT INTO mock_rule (id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response)
                VALUES ('rule-old', 'svc-1', '/api/old', 0, 'GET', 1, 0, '{"code":200}');

                INSERT INTO mock_rule (id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response)
                VALUES ('rule-new', 'svc-1', '/api/new', 0, 'GET', 1, 0, '{"code":200}');
                "#,
            )
            .unwrap();
        }

        // 打开仓库触发迁移，回填 created_at 后应保持「后插入者在前」的确定性顺序
        let repo = SqliteRepository::new(&path).unwrap();
        let rules = repo.list_rules("svc-1").await.unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].id, "rule-new");
        assert_eq!(rules[1].id, "rule-old");

        let _ = std::fs::remove_file(path);
    }
}
