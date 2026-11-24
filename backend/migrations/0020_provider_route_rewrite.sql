-- Built-in provider/route request rewriting: static outbound headers,
-- top-level parameter regex replacement, system-only prompt regex replacement,
-- and only-if-absent structured parameter defaults. Each column stores a JSON
-- array; an empty array means no rewriting. Route rules override provider rules
-- by key (header name / parameter field / prompt pattern) at relay time.
ALTER TABLE providers ADD COLUMN headers TEXT NOT NULL DEFAULT '[]';
ALTER TABLE providers ADD COLUMN param_replace TEXT NOT NULL DEFAULT '[]';
ALTER TABLE providers ADD COLUMN prompt_replace TEXT NOT NULL DEFAULT '[]';
ALTER TABLE providers ADD COLUMN param_defaults TEXT NOT NULL DEFAULT '[]';

ALTER TABLE routes ADD COLUMN headers TEXT NOT NULL DEFAULT '[]';
ALTER TABLE routes ADD COLUMN param_replace TEXT NOT NULL DEFAULT '[]';
ALTER TABLE routes ADD COLUMN prompt_replace TEXT NOT NULL DEFAULT '[]';
ALTER TABLE routes ADD COLUMN param_defaults TEXT NOT NULL DEFAULT '[]';