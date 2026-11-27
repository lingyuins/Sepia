-- Cache-read pricing (Issue #6): cached input tokens bill at a separate
-- per-token price. 0 = unset (falls back to the normal input price).
ALTER TABLE provider_models ADD COLUMN cache_read_price_micros INTEGER NOT NULL DEFAULT 0;
ALTER TABLE model_metadata ADD COLUMN cache_read_price_nanos INTEGER;