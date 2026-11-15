# Contract for working on Crumbs

This `AGENTS.md` is your binding contract for working on Crumbs. It uses keywords from RFC 2119 (MUST, MUST NOT, SHOULD, SHOULD NOT, etc.). You MUST obey this contract.

## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues for `alexandru/crumbs`. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: root `GLOSSARY.md` and `docs/adr/`. See `docs/agents/domain.md`.

## Coding standards

Rust code MUST follow CODING_STANDARDS.md.

### Rust unit tests

Unit tests MUST live in crate-local `unit-tests/`, mirroring `src/`, loaded through `mod tests` with `#[cfg(test)]` and a relative `#[path]`. Integration tests stay in `tests/`.

### Documentation

- You MUST NOT add documentation (markdown files for example) or code comments unless the user or a skill requests them.
- You MAY update existing documentation or comments that your code changes make incorrect.

