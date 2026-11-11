use chrono::{DateTime, Timelike, Utc};
use sqlx::FromRow;
use sqlx::QueryBuilder;
use sqlx::sqlite::Sqlite;

use crate::clock::UnixMillis;
use crate::identity::UserId;
use crate::profiles::{IanaTimezone, ProfileData, ProfileId};
use crate::runtime::Database;

use super::error::ProfileLogError;
use super::model::{
    BiometricEntry, BiometricLogEntry, CreateLog, LocalTime, LogDay, LogFilter, LogPage, PatchLog,
    ProfileLog, ProfileLogData, ProfileLogId,
};
use super::store::ProfileLogs;

const PAGE_SIZE: i64 = 100;

impl ProfileLogs for Database {
    async fn create_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        input: &CreateLog,
    ) -> Result<Option<ProfileLog>, ProfileLogError> {
        let mut transaction = begin_immediate(self).await?;

        let Some(owned_profile) = OwnedProfile::load(&mut transaction, user, profile).await? else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(None);
        };

        let now = self.clock.now().map_err(ProfileLogError::clock)?;
        let time = match input.time {
            Some(time) => time,
            None => local_time_at(owned_profile.data.settings.timezone, now)?,
        };
        let data = input.entry.into_data();
        let data_json = encode_data(&data)?;

        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO profile_logs (
                 profile_id, day, time, data_json, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, NULL)
             RETURNING id",
        )
        .bind(profile.get())
        .bind(input.day.to_string())
        .bind(time.to_string())
        .bind(data_json)
        .bind(now)
        .fetch_one(&mut *transaction)
        .await
        .map_err(ProfileLogError::storage)?;
        let id = ProfileLogId::new(id).map_err(|_| ProfileLogError::InvalidStoredData)?;

        owned_profile
            .update_current_weight_if_changed(&mut transaction, now)
            .await?;

        transaction
            .commit()
            .await
            .map_err(ProfileLogError::storage)?;

        Ok(Some(ProfileLog {
            id,
            profile_id: profile,
            day: input.day,
            time,
            data,
        }))
    }

    async fn profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
    ) -> Result<Option<ProfileLog>, ProfileLogError> {
        let row = sqlx::query_as::<_, ProfileLogRow>(
            "SELECT l.id, l.profile_id, l.day, l.time, l.data_json
             FROM profile_logs AS l
             JOIN profiles AS p ON p.id = l.profile_id
             WHERE p.user_id = ? AND p.id = ? AND l.id = ?",
        )
        .bind(user.get())
        .bind(profile.get())
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(ProfileLogError::storage)?;

        row.map(ProfileLogRow::into_log).transpose()
    }

    async fn list_profile_logs(
        &self,
        user: UserId,
        profile: ProfileId,
        filter: &LogFilter,
    ) -> Result<Option<LogPage>, ProfileLogError> {
        if !profile_owned(&self.pool, user, profile).await? {
            return Ok(None);
        }

        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, profile_id, day, time, data_json
             FROM profile_logs WHERE profile_id = ",
        );
        query.push_bind(profile.get());
        if let Some(from) = filter.from() {
            query.push(" AND day >= ").push_bind(from.to_string());
        }
        if let Some(to) = filter.to() {
            query.push(" AND day <= ").push_bind(to.to_string());
        }
        if let Some(entry_type) = filter.entry_type() {
            query
                .push(" AND entry_type = ")
                .push_bind(entry_type.as_str());
        }
        if let Some(subtype) = filter.subtype() {
            query.push(" AND subtype = ").push_bind(subtype.as_str());
        }
        if let Some(last_id) = filter.last_id() {
            query.push(" AND id < ").push_bind(last_id.get());
        }
        query.push(" ORDER BY id DESC LIMIT ").push_bind(PAGE_SIZE);

        let rows = query
            .build_query_as::<ProfileLogRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(ProfileLogError::storage)?;

        let items = rows
            .into_iter()
            .map(ProfileLogRow::into_log)
            .collect::<Result<Vec<_>, _>>()?;
        let last_id = items.last().map(|item| item.id);

        Ok(Some(LogPage { items, last_id }))
    }

    async fn patch_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
        patch: &PatchLog,
    ) -> Result<Option<ProfileLog>, ProfileLogError> {
        let mut transaction = begin_immediate(self).await?;

        let Some(owned_profile) = OwnedProfile::load(&mut transaction, user, profile).await? else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(None);
        };

        let Some(current) = owned_profile.load_log(&mut transaction, id).await? else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(None);
        };

        let updated = current.patched(patch);
        if updated == current {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(Some(current));
        }

        let now = self.clock.now().map_err(ProfileLogError::clock)?;
        sqlx::query(
            "UPDATE profile_logs
             SET day = ?, time = ?, data_json = ?, updated_at = ?
             WHERE profile_id = ? AND id = ?",
        )
        .bind(updated.day.to_string())
        .bind(updated.time.to_string())
        .bind(encode_data(&updated.data)?)
        .bind(now)
        .bind(profile.get())
        .bind(id.get())
        .execute(&mut *transaction)
        .await
        .map_err(ProfileLogError::storage)?;

        owned_profile
            .update_current_weight_if_changed(&mut transaction, now)
            .await?;

        transaction
            .commit()
            .await
            .map_err(ProfileLogError::storage)?;

        Ok(Some(updated))
    }

    async fn delete_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
    ) -> Result<bool, ProfileLogError> {
        let mut transaction = begin_immediate(self).await?;

        let Some(owned_profile) = OwnedProfile::load(&mut transaction, user, profile).await? else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(false);
        };

        if owned_profile
            .load_log(&mut transaction, id)
            .await?
            .is_none()
        {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(false);
        }

        sqlx::query("DELETE FROM profile_logs WHERE profile_id = ? AND id = ?")
            .bind(profile.get())
            .bind(id.get())
            .execute(&mut *transaction)
            .await
            .map_err(ProfileLogError::storage)?;

        let now = self.clock.now().map_err(ProfileLogError::clock)?;
        owned_profile
            .update_current_weight_if_changed(&mut transaction, now)
            .await?;

        transaction
            .commit()
            .await
            .map_err(ProfileLogError::storage)?;

        Ok(true)
    }
}

struct OwnedProfile {
    user: UserId,
    id: ProfileId,
    data: ProfileData,
}

impl OwnedProfile {
    async fn load(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        user: UserId,
        profile: ProfileId,
    ) -> Result<Option<Self>, ProfileLogError> {
        let data_json = sqlx::query_scalar::<_, String>(
            "SELECT data_json FROM profiles WHERE id = ? AND user_id = ?",
        )
        .bind(profile.get())
        .bind(user.get())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(ProfileLogError::storage)?;

        data_json
            .map(|json| {
                let data = serde_json::from_str::<ProfileData>(&json)
                    .map_err(|_| ProfileLogError::InvalidStoredData)?;
                Ok(Self {
                    user,
                    id: profile,
                    data,
                })
            })
            .transpose()
    }

    async fn load_log(
        &self,
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        id: ProfileLogId,
    ) -> Result<Option<ProfileLog>, ProfileLogError> {
        let row = sqlx::query_as::<_, ProfileLogRow>(
            "SELECT id, profile_id, day, time, data_json
             FROM profile_logs WHERE profile_id = ? AND id = ?",
        )
        .bind(self.id.get())
        .bind(id.get())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(ProfileLogError::storage)?;

        row.map(ProfileLogRow::into_log).transpose()
    }

    async fn update_current_weight_if_changed(
        mut self,
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        now: UnixMillis,
    ) -> Result<(), ProfileLogError> {
        let latest_json = sqlx::query_scalar::<_, String>(
            "SELECT data_json
             FROM profile_logs
             WHERE profile_id = ? AND entry_type = 'biometric' AND subtype = 'weight'
             ORDER BY day DESC, time DESC, created_at DESC, id DESC
             LIMIT 1",
        )
        .bind(self.id.get())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(ProfileLogError::storage)?;

        let latest = latest_json
            .map(|json| {
                let data = decode_data(&json)?;
                match data {
                    ProfileLogData::Biometric(BiometricLogEntry {
                        measurement: BiometricEntry::Weight(measurement),
                    }) => Ok(measurement.value),
                }
            })
            .transpose()?;

        if self.data.cache.current_weight == latest {
            return Ok(());
        }

        self.data.cache.current_weight = latest;
        let data_json = serde_json::to_string(&self.data).map_err(ProfileLogError::storage)?;

        let result = sqlx::query(
            "UPDATE profiles SET data_json = ?, updated_at = ?
             WHERE id = ? AND user_id = ?",
        )
        .bind(data_json)
        .bind(now)
        .bind(self.id.get())
        .bind(self.user.get())
        .execute(&mut **transaction)
        .await
        .map_err(ProfileLogError::storage)?;
        if result.rows_affected() != 1 {
            return Err(ProfileLogError::InvalidStoredData);
        }

        Ok(())
    }
}

async fn begin_immediate(
    database: &Database,
) -> Result<sqlx::Transaction<'static, Sqlite>, ProfileLogError> {
    database
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(ProfileLogError::storage)
}

async fn profile_owned(
    pool: &sqlx::SqlitePool,
    user: UserId,
    profile: ProfileId,
) -> Result<bool, ProfileLogError> {
    let found = sqlx::query_scalar::<_, i64>("SELECT 1 FROM profiles WHERE id = ? AND user_id = ?")
        .bind(profile.get())
        .bind(user.get())
        .fetch_optional(pool)
        .await
        .map_err(ProfileLogError::storage)?;
    Ok(found.is_some())
}

fn local_time_at(
    timezone: IanaTimezone,
    timestamp: UnixMillis,
) -> Result<LocalTime, ProfileLogError> {
    let utc = DateTime::<Utc>::from_timestamp_millis(timestamp).ok_or_else(|| {
        ProfileLogError::clock(std::io::Error::other(
            "application clock timestamp is outside the supported range",
        ))
    })?;
    let local = utc.with_timezone(&timezone.timezone()).time();
    LocalTime::new(
        local.hour(),
        local.minute(),
        local.second(),
        local.nanosecond() / 1_000_000,
    )
}

fn encode_data(data: &ProfileLogData) -> Result<String, ProfileLogError> {
    serde_json::to_string(data).map_err(ProfileLogError::storage)
}

fn decode_data(json: &str) -> Result<ProfileLogData, ProfileLogError> {
    serde_json::from_str(json).map_err(|_| ProfileLogError::InvalidStoredData)
}

#[derive(FromRow)]
struct ProfileLogRow {
    id: i64,
    profile_id: i64,
    day: String,
    time: String,
    data_json: String,
}

impl ProfileLogRow {
    fn into_log(self) -> Result<ProfileLog, ProfileLogError> {
        let id = ProfileLogId::new(self.id).map_err(|_| ProfileLogError::InvalidStoredData)?;
        let profile_id =
            ProfileId::new(self.profile_id).map_err(|_| ProfileLogError::InvalidStoredData)?;
        let day = LogDay::parse(&self.day).map_err(|_| ProfileLogError::InvalidStoredData)?;
        let time = LocalTime::parse(&self.time).map_err(|_| ProfileLogError::InvalidStoredData)?;
        let data = decode_data(&self.data_json)?;
        Ok(ProfileLog {
            id,
            profile_id,
            day,
            time,
            data,
        })
    }
}
