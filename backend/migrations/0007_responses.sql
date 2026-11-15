CREATE TABLE api_responses (
  id TEXT PRIMARY KEY,
  client_key_id INTEGER NOT NULL REFERENCES client_keys(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  status TEXT NOT NULL,
  background INTEGER NOT NULL DEFAULT 0,
  conversation_id TEXT,
  previous_response_id TEXT,
  request_json TEXT NOT NULL,
  response_json TEXT,
  input_items_json TEXT NOT NULL,
  output_items_json TEXT NOT NULL DEFAULT '[]',
  stream_events_json TEXT NOT NULL DEFAULT '[]',
  cancel_requested INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX api_responses_client ON api_responses(client_key_id, created_at);
CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  client_key_id INTEGER NOT NULL REFERENCES client_keys(id) ON DELETE CASCADE,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE conversation_items (
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  item_id TEXT,
  item_json TEXT NOT NULL,
  PRIMARY KEY (conversation_id, position)
);
CREATE INDEX conversation_items_id ON conversation_items(conversation_id, item_id);
