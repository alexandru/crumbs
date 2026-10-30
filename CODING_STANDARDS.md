# Coding standards

These rules apply to the Rust code in this repository and use RFC 2119 keywords.

Use the `rust-skills` and `code-refinements` skills when writing, reviewing, or refactoring Rust code.

## Types

Configuration fields and other domain values MUST use semantic types (e.g. `HttpPort`, `HttpStatusCode`), not bare primitives. Type aliases are the minimum; use newtypes when compiler-enforced distinctions or invariants are needed.

## Panics

Rust code MUST NOT panic. Agents MUST NOT use `unwrap`, `expect`, `panic!`, `todo!`, or `unreachable!`.
Async subsystems SHOULD share the application's runtime. The composition root MUST enable the drivers required by its subsystems and propagate runtime-creation failures as typed `Result` errors. Subsystem APIs MAY assume this configured application context, but MUST report a missing runtime through typed errors. Supporting arbitrary externally configured runtimes requires a separate explicit requirement.

## Errors

Fallible operations MUST return `Result` and use `?` to propagate errors.

## Imports

Agents SHOULD import types, traits, and constants freely. Agents SHOULD be more conservative about importing free functions and SHOULD import a free function only when the file uses it repeatedly. A function used once stays qualified with its module path.

```rust
use hex::FromHexError;

fn parse(input: &str) -> Result<Vec<u8>, FromHexError> {
    let bytes = hex::decode(input)?;
    // ...
    Ok(bytes)
}
```

## File structure

Files MUST start with the most important definitions: public types, their impls, and public functions. Private items follow, so the file reads from most important to least important.

## Comments

Comments MUST explain what the code does not make obvious and MUST NOT restate signatures or logic. Comments explaining non-obvious constructs (e.g., concurrency or lifecycle issues) to a Rust advanced-beginner are encouraged (assume expertise in Scala).
Public API documentation MUST state concise contracts, observable behavior, and error conditions. Explanations of Rust mechanics for an advanced beginner belong in inline comments, not API documentation.
