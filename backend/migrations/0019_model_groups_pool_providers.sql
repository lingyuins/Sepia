-- Model groups: reusable named sets of route slugs / model-name globs.
-- A client key's effective model scope is the union of its own `models`
-- and every referenced group's `members`. Groups only widen authorization;
-- they never change route selection, candidate order or upstream model names.
CREATE TABLE IF NOT EXISTS model_groups (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL DEFAULT '',
  members TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
ALTER TABLE client_key_policies ADD COLUMN model_groups TEXT NOT NULL DEFAULT '[]';
-- AI service pool entries may reuse an existing gateway provider instead of
-- a manually entered endpoint/key. NULL keeps the legacy custom-service form.
ALTER TABLE ai_pool_services ADD COLUMN provider_id INTEGER;