ALTER TABLE mock_rule ADD COLUMN response_variants TEXT NOT NULL DEFAULT '[]';
ALTER TABLE mock_rule ADD COLUMN selected_variant_id TEXT;