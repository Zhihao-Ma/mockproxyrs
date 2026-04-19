use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};
use tokio::sync::RwLock;

use super::MockRepository;
use crate::domain::{Method, MockRule, MockService};
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
        let conn = pool
            .get()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        Self::init_tables(&conn)?;

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

        let conn = pool
            .get()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;
        Self::init_tables(&conn)?;

        Ok(Self {
            pool,
            write_lock: Arc::new(RwLock::new(())),
        })
    }

    fn init_tables(conn: &rusqlite::Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS mock_service (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                listen_addr TEXT NOT NULL,
                target_url TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mock_rule (
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

            CREATE INDEX IF NOT EXISTS idx_mock_rule_service_id ON mock_rule(service_id);
            "#,
        )
        .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(())
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
                "SELECT id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response
                 FROM mock_rule WHERE service_id = ?",
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        let rules = stmt
            .query_map(params![service_id], |row| {
                let method: String = row.get(4)?;
                let is_regex: i32 = row.get(3)?;
                let enabled: i32 = row.get(5)?;
                let forward_and_record: i32 = row.get(6)?;
                Ok(MockRule {
                    id: row.get(0)?,
                    service_id: row.get(1)?,
                    url_pattern: row.get(2)?,
                    is_regex: is_regex != 0,
                    method: Self::parse_method(&method),
                    enabled: enabled != 0,
                    forward_and_record: forward_and_record != 0,
                    mock_response: row.get(7)?,
                })
            })
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(rules)
    }

    async fn get_rule(&self, id: &str) -> Result<Option<MockRule>> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response
                 FROM mock_rule WHERE id = ?",
            )
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        let result = stmt
            .query_row(params![id], |row| {
                let method: String = row.get(4)?;
                let is_regex: i32 = row.get(3)?;
                let enabled: i32 = row.get(5)?;
                let forward_and_record: i32 = row.get(6)?;
                Ok(MockRule {
                    id: row.get(0)?,
                    service_id: row.get(1)?,
                    url_pattern: row.get(2)?,
                    is_regex: is_regex != 0,
                    method: Self::parse_method(&method),
                    enabled: enabled != 0,
                    forward_and_record: forward_and_record != 0,
                    mock_response: row.get(7)?,
                })
            })
            .optional()
            .map_err(|e| MockproxyrsError::Database(e.to_string()))?;

        Ok(result)
    }

    async fn save_rule(&self, rule: &MockRule) -> Result<()> {
        let _lock = self.write_lock.write().await;
        let conn = self.get_conn()?;

        let method = Self::method_to_string(&rule.method);
        conn.execute(
            r#"INSERT INTO mock_rule (id, service_id, url_pattern, is_regex, method, enabled, forward_and_record, mock_response)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
               ON CONFLICT(id) DO UPDATE SET
                   url_pattern = excluded.url_pattern,
                   is_regex = excluded.is_regex,
                   method = excluded.method,
                   enabled = excluded.enabled,
                   forward_and_record = excluded.forward_and_record,
                   mock_response = excluded.mock_response"#,
            params![
                rule.id,
                rule.service_id,
                rule.url_pattern,
                rule.is_regex as i32,
                method,
                rule.enabled as i32,
                rule.forward_and_record as i32,
                rule.mock_response
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
            );
            repo.save_rule(&rule).await.unwrap();
        }

        let rules = repo.list_rules("svc-1").await.unwrap();
        assert_eq!(rules.len(), 10);
    }
}
