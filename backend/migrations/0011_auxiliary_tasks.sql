CREATE TABLE auxiliary_tasks (
  task_id TEXT PRIMARY KEY,
  client_key_id INTEGER NOT NULL REFERENCES client_keys(id) ON DELETE CASCADE,
  provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  provider_key_id INTEGER REFERENCES provider_keys(id) ON DELETE SET NULL,
  kind TEXT NOT NULL CHECK (kind IN ('video', 'suno')),
  model TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX auxiliary_tasks_client ON auxiliary_tasks(client_key_id, created_at);