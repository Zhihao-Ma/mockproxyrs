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
