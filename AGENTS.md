## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues for `alexandru/crumbs`. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: root `GLOSSARY.md` and `docs/adr/`. See `docs/agents/domain.md`.

## Coding standards

Rust code MUST follow CODING_STANDARDS.md.

### Rust unit tests

Rust unit tests MUST live in a crate-local `unit-tests/` directory that mirrors the `src/` layout, not inline in source files. A source file declares its test module with `#[cfg(test)]` and a `#[path]` attribute relative to that source file, and the module MUST be named `tests`:

```rust
#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
```

For `src/lib.rs` the test file is `unit-tests/lib.rs`. For `src/foo/bar.rs` it is `unit-tests/foo/bar.rs`, loaded with `#[path = "../../unit-tests/foo/bar.rs"]`. Integration tests MUST remain in `tests/`.
