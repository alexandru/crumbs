use std::future::Future;

use crate::identity::UserId;

use super::{
    CreateProfile, PatchProfile, Profile, ProfileError, ProfileId, ProfileList, ProfileListQuery,
};

/// Reads and manages profiles owned by an authenticated account.
pub trait Profiles: Sync {
    /// Creates a profile for `user`.
    ///
    /// # Errors
    ///
    /// Returns a validation, duplicate-name, clock, or storage error.
    fn create_profile(
        &self,
        user: UserId,
        input: &CreateProfile,
    ) -> impl Future<Output = Result<Profile, ProfileError>> + Send;

    /// Finds a profile owned by `user`.
    ///
    /// Returns `None` when the ID is absent or belongs to another user.
    ///
    /// # Errors
    ///
    /// Returns a storage or stored-data error.
    fn profile(
        &self,
        user: UserId,
        id: ProfileId,
    ) -> impl Future<Output = Result<Option<Profile>, ProfileError>> + Send;

    /// Lists the profiles owned by `user` in ascending ID order.
    ///
    /// # Errors
    ///
    /// Returns a storage or stored-data error.
    fn list_profiles(
        &self,
        user: UserId,
        query: &ProfileListQuery,
    ) -> impl Future<Output = Result<ProfileList, ProfileError>> + Send;

    /// Applies a partial update to a profile owned by `user`.
    ///
    /// Returns `None` when the ID is absent or belongs to another user. An
    /// update that leaves the profile unchanged does not change its timestamp.
    ///
    /// # Errors
    ///
    /// Returns a validation, duplicate-name, clock, storage, or stored-data error.
    fn patch_profile(
        &self,
        user: UserId,
        id: ProfileId,
        patch: &PatchProfile,
    ) -> impl Future<Output = Result<Option<Profile>, ProfileError>> + Send;
}
