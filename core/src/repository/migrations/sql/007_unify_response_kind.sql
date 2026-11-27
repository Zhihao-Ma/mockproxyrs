-- 每条响应（规则默认响应 / 响应变体）统一为单一内容槽 + 类型（静态 / JS 脚本）。
-- 规则：script 列并入 mock_response，use_script 改为表示默认响应的类型。

-- 脚本模式但脚本是空的：这种状态原本就无法工作，退回静态模式保留原 JSON
UPDATE mock_rule SET use_script = 0 WHERE use_script = 1 AND (script IS NULL OR trim(script) = '');
-- 其余脚本模式：脚本内容成为默认响应内容，未生效的 JSON 丢弃（单内容槽语义）
UPDATE mock_rule SET mock_response = script WHERE use_script = 1;
ALTER TABLE mock_rule DROP COLUMN script;

-- 变体支持脚本：加类型列，内容列改名
ALTER TABLE mock_response_variant ADD COLUMN kind TEXT NOT NULL DEFAULT 'static';
ALTER TABLE mock_response_variant RENAME COLUMN mock_response TO content;
