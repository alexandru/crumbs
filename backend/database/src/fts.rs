//! Full-text index invariants for `crumbs_foods`.
//!
//! `crumbs_foods_fts` is an external-content FTS5 table over `crumbs_foods`.
//! It stores token terms, not a duplicate copy of each food row; display text
//! remains in `crumbs_foods`. `content_rowid` is `crumbs_foods.id`, and the
//! insert, delete, and changed-field update triggers keep the index row in sync
//! with the food row in the same transaction.
//!
//! The update trigger compares `id`, `description`, `category`, and `tags` with
//! `IS NOT` so unchanged upserts do not rewrite index entries. The tokenizer is
//! `unicode61 remove_diacritics 2`: it folds case and Latin diacritics, but it
//! does not stem, translate, or treat different letters such as `â` and `î` as
//! interchangeable.

#[cfg(test)]
#[path = "../unit-tests/fts.rs"]
mod tests;
