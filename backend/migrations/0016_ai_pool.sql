CREATE TABLE IF NOT EXISTS ai_pool_services (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  nonce TEXT NOT NULL,
  ciphertext TEXT NOT NULL,
  model TEXT NOT NULL DEFAULT '',
  models TEXT NOT NULL DEFAULT '[]',
  timeout_seconds INTEGER NOT NULL DEFAULT 60,
  concurrency INTEGER NOT NULL DEFAULT 2,
  enabled INTEGER NOT NULL DEFAULT 1,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
