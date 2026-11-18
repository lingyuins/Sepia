CREATE TABLE IF NOT EXISTS model_metadata (
  model TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  source_id TEXT NOT NULL,
  base_model TEXT NOT NULL,
  display_name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  context_length INTEGER NOT NULL DEFAULT 0,
  input_modalities TEXT NOT NULL DEFAULT '[]',
  output_modalities TEXT NOT NULL DEFAULT '[]',
  capabilities TEXT NOT NULL DEFAULT '[]',
  input_price_nanos INTEGER,
  output_price_nanos INTEGER,
  fetched_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS catalog_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);