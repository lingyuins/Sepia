CREATE TABLE client_key_policies (
  client_key_id INTEGER PRIMARY KEY REFERENCES client_keys(id) ON DELETE CASCADE,
  ip_whitelist TEXT NOT NULL DEFAULT '[]',
  model_quotas TEXT NOT NULL DEFAULT '{}',
  allowed_groups TEXT NOT NULL DEFAULT '[]',
  excluded_providers TEXT NOT NULL DEFAULT '[]'
);
