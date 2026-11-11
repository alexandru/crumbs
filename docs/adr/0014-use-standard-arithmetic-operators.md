---
status: accepted
---

# Use standard arithmetic operators

Nutrition and profile formulas use ordinary operators and Rust's `Add`, `Sub`, `Mul`, `Div`, and `Neg` traits. Operators return values directly. Quantities keep their typed units and fixed-point decimal representation. Use exact decimal literals such as `dec!(2.5)` for constants.

We accept the remote risk of overflow in a 96-bit decimal coefficient. It does not justify checked-arithmetic helpers, `Result` wrappers, or artificial input limits. Keep formulas readable.

Parsing and domain validation still enforce positive serving amounts and compatible units. This decision supersedes the checked-arithmetic requirements in [issue #27](https://github.com/alexandru/crumbs/issues/27) and takes precedence over generic skill guidance.
