use sqlx::FromRow;
use sqlx::error::DatabaseError;

use crate::clock::UnixMillis;
use crate::identity::UserId;
use crate::runtime::Database;

use super::error::ProfileError;
use super::model::{
    CreateProfile, PatchProfile, Profile, ProfileData, ProfileId, ProfileList, ProfileListQuery,
    ProfileName, apply_patch, derive_profile,
};
use super::store::Profiles;

impl Profiles for Database {
    async fn create_profile(
        &self,
        user: UserId,
        input: &CreateProfile,
    ) -> Result<Profile, ProfileError> {
        let created_at = self.clock.now().map_err(ProfileError::clock)?;
        let name = &input.name;
        let data = ProfileData {
            settings: input.settings.clone().unwrap_or_default().into_settings(),
            cache: Default::default(),
        };
        let derived = derive_profile(&data.settings, &data.cache, created_at)?;
        let data_json = serde_json::to_string(&data).map_err(ProfileError::storage)?;
        let result = sqlx::query_scalar::<_, i64>(
            "INSERT INTO profiles (user_id, name, name_key, data_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, NULL)
             RETURNING id",
        )
        .bind(user.get())
        .bind(name.as_str())
        .bind(name.key())
        .bind(data_json)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await;
        let id = match result {
            Ok(id) => id,
            Err(error)
                if error
                    .as_database_error()
                    .is_some_and(DatabaseError::is_unique_violation) =>
            {
                return Err(ProfileError::DuplicateName);
            }
            Err(error) => return Err(ProfileError::storage(error)),
        };
        let id = ProfileId::new(id).map_err(|_| ProfileError::InvalidStoredData)?;
        Ok(Profile {
            id,
            name: name.clone(),
            data,
            derived,
        })
    }

    async fn profile(&self, user: UserId, id: ProfileId) -> Result<Option<Profile>, ProfileError> {
        let row = sqlx::query_as::<_, ProfileRow>(
            "SELECT id, name, name_key, data_json
             FROM profiles
             WHERE user_id = ? AND id = ?",
        )
        .bind(user.get())
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(ProfileError::storage)?;
        match row {
            Some(row) => {
                let now = self.clock.now().map_err(ProfileError::clock)?;
                row.into_profile(now).map(Some)
            }
            None => Ok(None),
        }
    }

    async fn list_profiles(
        &self,
        user: UserId,
        query: &ProfileListQuery,
    ) -> Result<ProfileList, ProfileError> {
        let last_id = query.last_id.map(ProfileId::get);
        let rows = sqlx::query_as::<_, ProfileRow>(
            "SELECT id, name, name_key, data_json
             FROM profiles
             WHERE user_id = ? AND (? IS NULL OR id > ?)
             ORDER BY id ASC
             LIMIT ?",
        )
        .bind(user.get())
        .bind(last_id)
        .bind(last_id)
        .bind(query.effective_limit())
        .fetch_all(&self.pool)
        .await
        .map_err(ProfileError::storage)?;
        if rows.is_empty() {
            return Ok(ProfileList {
                items: Vec::new(),
                last_id: None,
            });
        }
        let now = self.clock.now().map_err(ProfileError::clock)?;
        let items = rows
            .into_iter()
            .map(|row| row.into_profile(now))
            .collect::<Result<Vec<_>, _>>()?;
        let last_id = items.last().map(|item| item.id);
        Ok(ProfileList { items, last_id })
    }

    async fn patch_profile(
        &self,
        user: UserId,
        id: ProfileId,
        patch: &PatchProfile,
    ) -> Result<Option<Profile>, ProfileError> {
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(ProfileError::storage)?;
        let row = sqlx::query_as::<_, ProfileRow>(
            "SELECT id, name, name_key, data_json
             FROM profiles
             WHERE user_id = ? AND id = ?",
        )
        .bind(user.get())
        .bind(id.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(ProfileError::storage)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(ProfileError::storage)?;
            return Ok(None);
        };
        let (stored_id, stored_name, data) = row.into_parts()?;
        let (name, data, changed) = apply_patch(&stored_name, &data, patch);
        let now = self.clock.now().map_err(ProfileError::clock)?;
        if changed {
            let data_json = serde_json::to_string(&data).map_err(ProfileError::storage)?;
            let update = sqlx::query(
                "UPDATE profiles
                 SET name = ?, name_key = ?, data_json = ?, updated_at = ?
                 WHERE user_id = ? AND id = ?",
            )
            .bind(name.as_str())
            .bind(name.key())
            .bind(data_json)
            .bind(now)
            .bind(user.get())
            .bind(stored_id.get())
            .execute(&mut *transaction)
            .await;
            if let Err(error) = update {
                if error
                    .as_database_error()
                    .is_some_and(DatabaseError::is_unique_violation)
                {
                    return Err(ProfileError::DuplicateName);
                }
                return Err(ProfileError::storage(error));
            }
        }
        let result = profile(stored_id, name, data, now)?;
        transaction.commit().await.map_err(ProfileError::storage)?;
        Ok(Some(result))
    }
}

fn profile(
    id: ProfileId,
    name: ProfileName,
    data: ProfileData,
    now: UnixMillis,
) -> Result<Profile, ProfileError> {
    let derived = derive_profile(&data.settings, &data.cache, now)?;
    Ok(Profile {
        id,
        name,
        data,
        derived,
    })
}

#[derive(FromRow)]
struct ProfileRow {
    id: i64,
    name: String,
    name_key: String,
    data_json: String,
}

impl ProfileRow {
    fn into_parts(self) -> Result<(ProfileId, ProfileName, ProfileData), ProfileError> {
        let id = ProfileId::new(self.id).map_err(|_| ProfileError::InvalidStoredData)?;
        let name = ProfileName::new(&self.name).map_err(|_| ProfileError::InvalidStoredData)?;
        if name.as_str() != self.name || name.key() != self.name_key {
            return Err(ProfileError::InvalidStoredData);
        }
        let data = serde_json::from_str::<ProfileData>(&self.data_json)
            .map_err(|_| ProfileError::InvalidStoredData)?;
        Ok((id, name, data))
    }

    fn into_profile(self, now: UnixMillis) -> Result<Profile, ProfileError> {
        let (id, name, data) = self.into_parts()?;
        profile(id, name, data, now)
    }
}
