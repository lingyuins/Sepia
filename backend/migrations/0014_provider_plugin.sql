-- Providers may be backed by a WASM plugin instead of a direct HTTP upstream.
ALTER TABLE providers ADD COLUMN plugin_id TEXT;