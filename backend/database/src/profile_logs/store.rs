use std::future::Future;

use crate::identity::UserId;
use crate::profiles::ProfileId;

use super::{CreateLog, LogFilter, LogPage, PatchLog, ProfileLog, ProfileLogError, ProfileLogId};

/// Reads and manages profile logs owned by an authenticated account.
pub trait ProfileLogs: Sync {
    /// Creates a food or biometric log for a profile owned by user.
    ///
    /// Returns None when the profile is absent or belongs to another user.
    ///
    /// # Errors
    ///
    /// Returns a validation, clock, storage, or stored-data error.
    fn create_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        input: &CreateLog,
    ) -> impl Future<Output = Result<Option<ProfileLog>, ProfileLogError>> + Send;

    /// Finds a log under a profile owned by user.
    ///
    /// Returns None when the profile or log is absent or belongs to another user.
    ///
    /// # Errors
    ///
    /// Returns a clock, storage, or stored-data error.
    fn profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
    ) -> impl Future<Output = Result<Option<ProfileLog>, ProfileLogError>> + Send;

    /// Lists logs under a profile owned by user, newest ID first.
    ///
    /// Returns None when the profile is absent or belongs to another user.
    ///
    /// # Errors
    ///
    /// Returns a storage or stored-data error.
    fn list_profile_logs(
        &self,
        user: UserId,
        profile: ProfileId,
        filter: &LogFilter,
    ) -> impl Future<Output = Result<Option<LogPage>, ProfileLogError>> + Send;

    /// Applies a partial update to a log under a profile owned by user.
    ///
    /// Returns None when the profile or log is absent or belongs to another user.
    ///
    /// # Errors
    ///
    /// Returns a validation, clock, storage, or stored-data error.
    fn patch_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
        patch: &PatchLog,
    ) -> impl Future<Output = Result<Option<ProfileLog>, ProfileLogError>> + Send;

    /// Deletes a log under a profile owned by user.
    ///
    /// Returns false when the profile or log is absent or belongs to another user.
    ///
    /// # Errors
    ///
    /// Returns a clock, storage, or stored-data error.
    fn delete_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
    ) -> impl Future<Output = Result<bool, ProfileLogError>> + Send;
}
