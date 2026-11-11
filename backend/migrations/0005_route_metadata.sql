ALTER TABLE routes ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE routes ADD COLUMN description TEXT NOT NULL DEFAULT '';
UPDATE routes SET name = slug WHERE name = '';
