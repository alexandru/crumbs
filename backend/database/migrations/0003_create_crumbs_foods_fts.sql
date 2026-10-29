CREATE VIRTUAL TABLE crumbs_foods_fts USING fts5(
    description,
    category,
    tags,
    content = 'crumbs_foods',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER crumbs_foods_fts_after_insert
AFTER INSERT ON crumbs_foods
BEGIN
    INSERT INTO crumbs_foods_fts (rowid, description, category, tags)
    VALUES (new.id, new.description, new.category, new.tags);
END;

CREATE TRIGGER crumbs_foods_fts_after_delete
AFTER DELETE ON crumbs_foods
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, description, category, tags)
    VALUES ('delete', old.id, old.description, old.category, old.tags);
END;

CREATE TRIGGER crumbs_foods_fts_after_update
AFTER UPDATE OF id, description, category, tags ON crumbs_foods
WHEN old.id IS NOT new.id
    OR old.description IS NOT new.description
    OR old.category IS NOT new.category
    OR old.tags IS NOT new.tags
BEGIN
    INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rowid, description, category, tags)
    VALUES ('delete', old.id, old.description, old.category, old.tags);
    INSERT INTO crumbs_foods_fts (rowid, description, category, tags)
    VALUES (new.id, new.description, new.category, new.tags);
END;

INSERT INTO crumbs_foods_fts (crumbs_foods_fts) VALUES ('rebuild');
