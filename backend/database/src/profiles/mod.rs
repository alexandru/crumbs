//! Account-owned personal profiles and calculated calorie values.

mod error;
mod model;
mod sqlite;
mod store;

pub use error::ProfileError;
pub use model::{
    ActivityLevel, BaselineActivity, BirthDate, BirthDatePrecision, BmrSetting, BodyHeight,
    BodyWeight, CreateProfile, CreateProfileSettings, CustomBmr, IanaTimezone, NullablePatch,
    Patch, PatchProfile, Profile, ProfileCache, ProfileData, ProfileDerived, ProfileId,
    ProfileLimit, ProfileList, ProfileListQuery, ProfileName, ProfileSettings, Sex,
    WeightChangeGoal,
};
pub use store::Profiles;

#[cfg(test)]
#[path = "../../unit-tests/profiles/mod.rs"]
mod tests;
