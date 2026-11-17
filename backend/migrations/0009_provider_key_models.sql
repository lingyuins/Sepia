-- Per-key model scope: a relay key may only serve a subset of the provider's
-- models (e.g. one Anthropic-class key and one OpenAI-class key on one relay).
-- NULL means the key serves every provider model (legacy behavior).
ALTER TABLE provider_keys ADD COLUMN models TEXT;