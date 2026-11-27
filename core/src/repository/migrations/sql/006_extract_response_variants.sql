-- 响应变体改为独立表存储：变体是规则的从属于对象，分表后对单条变体的增删改是行级操作，
-- 不会因为保存规则其它字段而被整列覆盖（多页面同时改数据时的数据冲突根源）。
-- 旧 JSON 列中的变体数据不迁移（测试版数据可丢弃），对应的 selected_variant_id 一并清空。
UPDATE mock_rule SET selected_variant_id = NULL;

CREATE TABLE mock_response_variant (
    id            TEXT PRIMARY KEY,
    rule_id       TEXT NOT NULL REFERENCES mock_rule(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    mock_response TEXT NOT NULL,
    position      INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_mock_response_variant_rule_id
    ON mock_response_variant(rule_id, position);

ALTER TABLE mock_rule DROP COLUMN response_variants;
