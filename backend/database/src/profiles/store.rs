use std::future::Future;

use crate::identity::UserId;

use super::{
    CreateProfile, PatchProfile, Profile, ProfileError, ProfileId, ProfileList, ProfileListQuery,
};

pub trait Profiles: Sync {
    fn create_profile(
        &self,
        user: UserId,
        input: &CreateProfile,
    ) -> impl Future<Output = Result<Profile, ProfileError>> + Send;

    fn profile(
        &self,
        user: UserId,
        id: ProfileId,
    ) -> impl Future<Output = Result<Option<Profile>, ProfileError>> + Send;

    fn list_profiles(
        &self,
        user: UserId,
        query: &ProfileListQuery,
    ) -> impl Future<Output = Result<ProfileList, ProfileError>> + Send;

    fn patch_profile(
        &self,
        user: UserId,
        id: ProfileId,
        patch: &PatchProfile,
    ) -> impl Future<Output = Result<Option<Profile>, ProfileError>> + Send;
}
