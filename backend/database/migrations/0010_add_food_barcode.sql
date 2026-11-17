ALTER TABLE crumbs_foods ADD COLUMN barcode TEXT;

DROP TRIGGER crumbs_foods_fts_after_insert;
DROP TRIGGER crumbs_foods_fts_before_delete;
DROP TRIGGER crumbs_foods_fts_before_update;
DROP TRIGGER crumbs_foods_fts_after_update;
DROP TRIGGER crumbs_food_tags_fts_before_insert;
DROP TRIGGER crumbs_food_tags_fts_after_insert;
DROP TRIGGER crumbs_food_tags_fts_before_delete;
DROP TRIGGER crumbs_food_tags_fts_after_delete;
DROP TRIGGER crumbs_food_tags_fts_before_update;
DROP TRIGGER crumbs_food_tags_fts_after_update;
DROP TRIGGER crumbs_tags_fts_before_delete;
DROP TRIGGER crumbs_tags_fts_before_update;
DROP TRIGGER crumbs_tags_fts_after_update;

DROP TABLE crumbs_foods_fts;
DROP VIEW crumbs_foods_search_content;

CREATE VIEW crumbs_foods_search_content AS
SELECT
    f.id,
    f.source,
    f.dataset,
    f.description,
    f.category,
    f.barcode,
    COALESCE(
        (
            SELECT group_concat(t.name, ' ' ORDER BY t.name_key)
            FROM crumbs_food_tags AS ft
            JOIN crumbs_tags AS t ON t.id = ft.tag_id
            WHERE ft.food_id = f.id
        ),
        ''
    ) AS tags
FROM crumbs_foods AS f;

CREATE VIRTUAL TABLE crumbs_foods_fts USING fts5(
    source,
    dataset,
    description,
    category,
    barcode,
    tags,
    content = 'crumbs_foods_search_content',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER crumbs_foods_fts_after_insert
AFTER INSERT ON crumbs_foods
BEGIN
    INSERT INTO crumbs_foods_fts (rowid, source, dataset, description, category, barcode, tags)
    SELECT id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = new.id;
END;

CREATE TRIGGER crumbs_foods_fts_before_delete
BEFORE DELETE ON crumbs_foods
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, source, dataset, description, category, barcode, tags)
    SELECT 'delete', id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = old.id;
END;

CREATE TRIGGER crumbs_foods_fts_before_update
BEFORE UPDATE OF id, source, dataset, description, category, barcode ON crumbs_foods
WHEN old.id IS NOT new.id
    OR old.source IS NOT new.source
    OR old.dataset IS NOT new.dataset
    OR old.description IS NOT new.description
    OR old.category IS NOT new.category
    OR old.barcode IS NOT new.barcode
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, source, dataset, description, category, barcode, tags)
    SELECT 'delete', id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = old.id;
END;

CREATE TRIGGER crumbs_foods_fts_after_update
AFTER UPDATE OF id, source, dataset, description, category, barcode ON crumbs_foods
WHEN old.id IS NOT new.id
    OR old.source IS NOT new.source
    OR old.dataset IS NOT new.dataset
    OR old.description IS NOT new.description
    OR old.category IS NOT new.category
    OR old.barcode IS NOT new.barcode
BEGIN
    INSERT INTO crumbs_foods_fts (rowid, source, dataset, description, category, barcode, tags)
    SELECT id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = new.id;
END;

CREATE TRIGGER crumbs_food_tags_fts_before_insert
BEFORE INSERT ON crumbs_food_tags
WHEN NOT EXISTS (
    SELECT 1 FROM crumbs_food_tags
    WHERE food_id = new.food_id AND tag_id = new.tag_id
)
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, source, dataset, description, category, barcode, tags)
    SELECT 'delete', id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = new.food_id;
END;

CREATE TRIGGER crumbs_food_tags_fts_after_insert
AFTER INSERT ON crumbs_food_tags
BEGIN
    INSERT INTO crumbs_foods_fts (rowid, source, dataset, description, category, barcode, tags)
    SELECT id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = new.food_id;
END;

CREATE TRIGGER crumbs_food_tags_fts_before_delete
BEFORE DELETE ON crumbs_food_tags
WHEN EXISTS (SELECT 1 FROM crumbs_foods WHERE id = old.food_id)
    AND EXISTS (SELECT 1 FROM crumbs_tags WHERE id = old.tag_id)
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, source, dataset, description, category, barcode, tags)
    SELECT 'delete', id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = old.food_id;
END;

CREATE TRIGGER crumbs_food_tags_fts_after_delete
AFTER DELETE ON crumbs_food_tags
WHEN EXISTS (SELECT 1 FROM crumbs_foods WHERE id = old.food_id)
    AND EXISTS (SELECT 1 FROM crumbs_tags WHERE id = old.tag_id)
BEGIN
    INSERT INTO crumbs_foods_fts (rowid, source, dataset, description, category, barcode, tags)
    SELECT id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id = old.food_id;
END;

CREATE TRIGGER crumbs_food_tags_fts_before_update
BEFORE UPDATE OF food_id, tag_id ON crumbs_food_tags
WHEN (old.food_id IS NOT new.food_id OR old.tag_id IS NOT new.tag_id)
    AND EXISTS (SELECT 1 FROM crumbs_foods WHERE id = old.food_id)
    AND EXISTS (SELECT 1 FROM crumbs_tags WHERE id = old.tag_id)
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, source, dataset, description, category, barcode, tags)
    SELECT 'delete', id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id IN (old.food_id, new.food_id);
END;

CREATE TRIGGER crumbs_food_tags_fts_after_update
AFTER UPDATE OF food_id, tag_id ON crumbs_food_tags
WHEN (old.food_id IS NOT new.food_id OR old.tag_id IS NOT new.tag_id)
    AND EXISTS (SELECT 1 FROM crumbs_foods WHERE id = old.food_id)
    AND EXISTS (SELECT 1 FROM crumbs_tags WHERE id = old.tag_id)
BEGIN
    INSERT INTO crumbs_foods_fts (rowid, source, dataset, description, category, barcode, tags)
    SELECT id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id IN (old.food_id, new.food_id);
END;

CREATE TRIGGER crumbs_tags_fts_before_delete
BEFORE DELETE ON crumbs_tags
BEGIN
    DELETE FROM crumbs_food_tags WHERE tag_id = old.id;
END;

CREATE TRIGGER crumbs_tags_fts_before_update
BEFORE UPDATE OF id, name, name_key ON crumbs_tags
WHEN old.id IS NOT new.id
    OR old.name IS NOT new.name
    OR old.name_key IS NOT new.name_key
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, source, dataset, description, category, barcode, tags)
    SELECT 'delete', id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id IN (SELECT food_id FROM crumbs_food_tags WHERE tag_id = old.id);
END;

CREATE TRIGGER crumbs_tags_fts_after_update
AFTER UPDATE OF id, name, name_key ON crumbs_tags
WHEN old.id IS NOT new.id
    OR old.name IS NOT new.name
    OR old.name_key IS NOT new.name_key
BEGIN
    INSERT INTO crumbs_foods_fts (rowid, source, dataset, description, category, barcode, tags)
    SELECT id, source, dataset, description, category, barcode, tags
    FROM crumbs_foods_search_content
    WHERE id IN (SELECT food_id FROM crumbs_food_tags WHERE tag_id = new.id);
END;

INSERT INTO crumbs_foods_fts (crumbs_foods_fts) VALUES ('rebuild');
