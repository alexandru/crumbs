use std::future::Future;

use crate::identity::UserId;
use crate::profiles::ProfileId;

use super::{
    CreateLog, CreateLogBatch, LogFilter, LogPage, PatchLog, ProfileLog, ProfileLogError,
    ProfileLogId,
};

pub trait ProfileLogs: Sync {
    fn create_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        input: &CreateLog,
    ) -> impl Future<Output = Result<Option<ProfileLog>, ProfileLogError>> + Send;

    fn create_profile_logs(
        &self,
        user: UserId,
        profile: ProfileId,
        input: &CreateLogBatch,
    ) -> impl Future<Output = Result<Option<Vec<ProfileLog>>, ProfileLogError>> + Send;

    fn profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
    ) -> impl Future<Output = Result<Option<ProfileLog>, ProfileLogError>> + Send;

    fn list_profile_logs(
        &self,
        user: UserId,
        profile: ProfileId,
        filter: &LogFilter,
    ) -> impl Future<Output = Result<Option<LogPage>, ProfileLogError>> + Send;

    fn patch_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
        patch: &PatchLog,
    ) -> impl Future<Output = Result<Option<ProfileLog>, ProfileLogError>> + Send;

    fn delete_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
    ) -> impl Future<Output = Result<bool, ProfileLogError>> + Send;
}
