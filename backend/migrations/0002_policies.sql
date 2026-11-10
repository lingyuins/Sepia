CREATE TABLE channel_groups (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL
);
ALTER TABLE providers ADD COLUMN group_id TEXT NOT NULL DEFAULT 'official';
CREATE TABLE workspace_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
