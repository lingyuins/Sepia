-- Richer request observation: client address, time to first upstream byte and
-- token detail (cached input / reasoning output). Bodies are opt-in via the
-- `log_bodies` workspace setting and live in their own table so the metadata
-- log stays small and clearing bodies never touches request records.
ALTER TABLE requests ADD COLUMN client_ip TEXT;
ALTER TABLE requests ADD COLUMN first_byte_ms INTEGER;
ALTER TABLE requests ADD COLUMN cached_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE requests ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0;
CREATE TABLE request_bodies (
  request_id TEXT PRIMARY KEY REFERENCES requests(id) ON DELETE CASCADE,
  request_body TEXT NOT NULL,
  response_body TEXT,
  created_at TEXT NOT NULL
);
