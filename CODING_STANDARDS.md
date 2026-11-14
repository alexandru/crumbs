# Coding standards

These rules apply to the Rust code in this repository and use RFC 2119 keywords.

Use the `rust-skills` and `code-refinements` skills when writing, reviewing, or refactoring Rust code.

## Types

Configuration fields and other domain values MUST use semantic types (e.g. `HttpPort`, `HttpStatusCode`), not bare primitives. Type aliases are the minimum; use newtypes when compiler-enforced distinctions or invariants are needed.

Domain types MUST make illegal states unrepresentable. Code MUST parse external values into semantic types that preserve their invariants ("parse, don't validate"). Public constructors and deserialization MUST enforce those invariants so callers cannot construct invalid values. Once parsed, downstream code MUST rely on the type rather than repeat validation.

All units of measurement MUST use distinct semantic newtypes (e.g. `Grams`, `Milligrams`, `Kcal`, `Kilojoules`), not bare primitives or type aliases. Measurement types MUST keep their representation private so callers cannot construct invalid quantities or mix units implicitly. Unit conversions MUST be explicit and typed.

Nutrition quantities and arithmetic MUST use fixed-point decimal representations. Parsing, conversion, storage, and serialization MUST NOT pass these quantities through binary floating-point values.

## Standard types and library APIs

Code SHOULD reuse standard types and existing library APIs when they meet the required semantics. Before implementing a custom representation, parser, or algorithm, verify what those APIs support and identify the unmet requirement. Domain constraints can be enforced by a newtype around an existing type, such as `std::time::Duration`.

## Arithmetic

Use ordinary operators and `std::ops` traits. Do not wrap arithmetic in overflow checks or `Result`. See [ADR-0014](docs/adr/0014-use-standard-arithmetic-operators.md).

```diff
-let total = amount.checked_scale(Decimal::new(25, 1))?;
+let total = amount * dec!(2.5);
```

## Construction

Numeric newtypes SHOULD implement `From` for common inputs that convert without loss or validation. Call sites SHOULD use these conversions directly, such as `KcalPerKg::from(10)`.

### Avoid repeated newtype implementations

`ExerciseType` repeated blank checks, constructors, accessors, and deserialization already needed by other text types. A custom declaration macro then hid ordinary Rust behind new syntax. Neither approach was necessary.

Newtypes SHOULD reuse validated inner types, such as `NonBlankString`, and derive forwarding constructors, accessors, and serialization instead of repeating those implementations. Wrapper constructors SHOULD accept unvalidated input and delegate validation to the inner type. Callers SHOULD NOT need to construct the inner value first.

Bodies are abbreviated:

```diff
-#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
+#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AliasTryNew, AliasValue)]
 #[serde(transparent)]
-pub struct ExerciseType(String);
-impl ExerciseType {
-    pub fn new(value: impl Into<String>) -> Result<Self, ProfileLogError> { ... }
-    pub fn as_str(&self) -> &str { ... }
-}
-impl<'de> Deserialize<'de> for ExerciseType { ... }
+pub struct ExerciseType(NonBlankString);
```

```diff
-let activity = ExerciseType(NonBlankString::new("walking")?);
+let activity = ExerciseType::new("walking")?;
+let name = activity.value().as_str();
```

## Enum payloads

Equivalent payload alternatives in a domain enum MUST use the same constructor style. Stored, create, and patch enums for the same categories MUST follow the same convention. When alternatives wrap category payload types, each variant MUST carry one payload through a tuple constructor; each payload’s named fields belong in its own struct. Unit variants and variants with different semantic roles MAY use different shapes.

Code MUST NOT add `Box` or other heap indirection solely to silence a size lint or without a demonstrated benefit for the expected workload. An enum intentionally stored inline MAY use a narrowly scoped lint allowance.

For example, the payload struct retains the existing `{"type":"biometric","measurement":...}` JSON representation:

```diff
+pub struct BiometricLogEntry {
+    pub measurement: BiometricEntry,
+}
+
pub enum ProfileLogData {
-    Biometric { measurement: BiometricEntry },
-    Food(Box<FoodLogEntry>),
+    Biometric(BiometricLogEntry),
+    Food(FoodLogEntry),
}

pub enum LogInput {
-    Biometric { measurement: BiometricEntry },
+    Biometric(BiometricLogEntry),
    Food(FoodLogInput),
}

pub enum LogEntryPatch {
-    Biometric { measurement: BiometricEntry },
+    Biometric(BiometricLogEntry),
    Food(FoodLogPatch),
}
```

## Panics

Rust code SHOULD avoid panics. Agents MUST NOT use `unwrap`, `expect`, `panic!`, `todo!`, or `unreachable!`.

## Errors

Fallible operations MUST return `Result` and use `?` to propagate errors.

## Async runtime

Async subsystems SHOULD share the application's runtime. The composition root MUST enable the drivers required by its subsystems and propagate runtime-creation failures as typed `Result` errors. Subsystem APIs MAY assume this configured application context. Supporting arbitrary externally configured runtimes requires a separate explicit requirement.

## Dependencies

Third-party dependencies SHOULD be widely used and actively maintained.

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

## Keep related data and behavior together

A function SHOULD live in a type’s inherent `impl` when that type is its natural receiver or the function builds values of that type. After refactoring, the function MUST refer to `Self` in its input parameters or return type, directly or as a type parameter. Keep these implementations adjacent to the type definition.

These examples show signatures only. Function bodies are omitted.

A builder returns the type it constructs:

```diff
-pub(crate) fn derive_profile(...) -> Result<ProfileDerived, ProfileError> {
-    ...
-}
+impl ProfileDerived {
+    pub(crate) fn calculate(...) -> Result<Self, ProfileError> {
+        ...
+    }
+}
```

A method calculates age from a birth date:

```diff
-fn age_on(birth_date: BirthDate, current_day: NaiveDate) -> Option<i32> {
-    ...
-}
+impl BirthDate {
+    fn age_on(&self, current_day: NaiveDate) -> Option<i32> {
+        ...
+    }
+}
```

## Code paragraphs

Function bodies MUST use blank lines to separate main ideas.

```diff
 let result = profile(stored_id, name, data, now)?;
+
 transaction.commit().await.map_err(ProfileError::storage)?;
 Ok(Some(result))
```

## Test compilation

Rust production code under crate-local `src/` MUST NOT use `#[cfg(not(test))]`. `#[cfg(test)]` is permitted under `src/` ONLY on module declarations that load mirrored crate-local `unit-tests/` modules through the required relative `#[path]`.

Production implementations MUST be the same in test and non-test builds. Test-only helpers MUST live in `unit-tests/`, not `src/`. Equivalent test-dependent `cfg`, `cfg_attr`, or `cfg!` forms MUST NOT bypass this rule.

Code reviews MUST explicitly verify compliance.

## Comments

Comments MUST explain what the code does not make obvious and MUST NOT restate signatures or logic. Comments explaining non-obvious constructs (e.g., concurrency or lifecycle issues) to a Rust advanced-beginner are encouraged (assume expertise in Scala).
Public API documentation MUST state concise contracts, observable behavior, and error conditions. Explanations of Rust mechanics for an advanced beginner belong in inline comments, not API documentation.
