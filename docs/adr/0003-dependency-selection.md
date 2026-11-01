# Prefer popular, well-maintained dependencies

Dependencies must be popular and well-maintained, with credible adoption and active support for their purpose. Evaluate actual downstream use, release and maintenance history, and responses to relevant correctness or data updates. Download counts and stars alone do not establish maintenance, and inactivity alone does not establish abandonment. Prefer an established, maintained option when it meets the same requirements without disproportionate cost.

For case-insensitive tag identity, use [`unicase` 2.10.0](https://crates.io/crates/unicase/2.10.0) with canonical Unicode normalization before and after full case folding. Ordinary lowercase is insufficient. Keep trimming, original display spelling, canonical equivalence, and meaningful accent distinctions.

As assessed on 2026-11-01, `unicase` has wider published adoption, including `clap_builder`, `mime_guess`, `publicsuffix`, and `minijinja`, and [recent Unicode data maintenance](https://github.com/seanmonstar/unicase/pull/86) released on 2026-10-05. [`caseless` 0.2.2](https://crates.io/crates/caseless/0.2.2) also has credible users, but its latest release is dated 2024-12-30 and its [Unicode 17 update](https://github.com/unicode-rs/rust-caseless/pull/25) remains open. Its Unicode 16 folding data misses newer mappings. The combination of wider adoption and current Unicode support favors `unicase`; this is not a claim that `caseless` is abandoned.

`unicase` does not perform canonical normalization itself, so `unicode-normalization` remains part of tag-key construction. Folding data updates can change identity keys for newly supported characters and require consideration before upgrading a deployed database.
