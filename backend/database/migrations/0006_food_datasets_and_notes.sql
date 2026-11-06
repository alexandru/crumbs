ALTER TABLE crumbs_foods ADD COLUMN notes TEXT;

UPDATE crumbs_foods
SET dataset = CASE dataset
    WHEN 'usda_foundation' THEN 'common'
    WHEN 'usda_sr_legacy' THEN 'common'
    WHEN 'usda_survey_fndds' THEN 'common'
    WHEN 'usda_branded' THEN 'branded'
    ELSE dataset
END
WHERE dataset IN (
    'usda_foundation',
    'usda_sr_legacy',
    'usda_survey_fndds',
    'usda_branded'
);

CREATE TEMP TABLE crumbs_dataset_migration_validation (
    dataset TEXT NOT NULL
        CONSTRAINT foods_dataset_must_be_common_branded_or_custom
        CHECK (dataset IN ('common', 'branded', 'custom'))
) STRICT;

INSERT INTO crumbs_dataset_migration_validation (dataset)
SELECT DISTINCT dataset FROM crumbs_foods;

DROP TABLE crumbs_dataset_migration_validation;
