//! Account-owned personal profiles and calculated calorie values.
//!
//! Calorie calculations use checked fixed-point decimal arithmetic, with at
//! most 28 fractional places subject to Decimal's 96-bit precision. When an
//! operation must reduce precision, midpoint values round to nearest-even.
//! Overflow returns [ProfileError::Calculation]. Calculations do not use
//! binary floating point or round to whole calories.

mod energy;
mod error;
mod model;
mod sqlite;
mod store;

pub use energy::KilocaloriesPerDay;
pub use error::ProfileError;
pub use model::{
    ActivityLevel, BaselineActivity, BirthDate, BirthDatePrecision, BmrPatch, BmrSetting,
    BodyHeight, BodyWeight, CreateProfile, CreateProfileSettings, CustomBmr, IanaTimezone,
    NullablePatch, Patch, PatchProfile, Profile, ProfileCache, ProfileData, ProfileDerived,
    ProfileId, ProfileLimit, ProfileList, ProfileListQuery, ProfileName, ProfileSettings, Sex,
};
pub use store::Profiles;

#[cfg(test)]
#[path = "../../unit-tests/profiles/mod.rs"]
mod tests;
