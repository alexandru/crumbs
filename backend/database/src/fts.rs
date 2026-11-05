//! Full-text index invariants for food metadata and normalized tag names.
//!
//! `crumbs_foods_fts` is an external-content FTS5 table over
//! `crumbs_foods_search_content`, a view projecting source, dataset, food
//! description, category, and ordered tag names from `crumbs_tags` and
//! `crumbs_food_tags`. Source data and display values remain in their normalized
//! tables; FTS stores token terms.
//! `content_rowid` is the view's food `id`.
//!
//! Food triggers reindex changed `id`, `source`, `dataset`, `description`, or
//! `category` values. Assignment and tag triggers reindex documents when tag
//! membership or names change. `IS NOT` comparisons avoid rewriting unchanged
//! metadata. The tokenizer is `unicode61 remove_diacritics 2`: it folds case
//! and Latin diacritics, but it does not stem, translate, or treat different
//! letters such as `â` and `î` as interchangeable.

#[cfg(test)]
#[path = "../unit-tests/fts.rs"]
mod tests;
