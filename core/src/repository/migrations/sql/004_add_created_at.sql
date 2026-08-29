ALTER TABLE mock_rule ADD COLUMN created_at INTEGER;

-- 回填存量行：以 rowid 作为稳定创建顺序，保证升级后老数据按插入顺序倒序、确定性排序
UPDATE mock_rule SET created_at = rowid WHERE created_at IS NULL;