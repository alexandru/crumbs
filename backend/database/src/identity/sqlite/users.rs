use sqlx::{FromRow, error::DatabaseError};

use crate::identity::users::{
    PasswordHash, User, UserCredentials, UserError, UserId, UserTimestamp, Username, Users,
};
use crate::runtime::Database;

use super::sessions::revoke_user_sessions;

impl Users for Database {
    async fn create_user(
        &self,
        username: &Username,
        password_hash: &PasswordHash,
    ) -> Result<User, UserError> {
        let created_at = self
            .clock
            .now()
            .map_err(|source| UserError::Clock(Box::new(source)))?;
        let result = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (username, password_hash, active, created_at)
             VALUES (?, ?, 1, ?)
             RETURNING id, username, active, created_at",
        )
        .bind(username.as_str())
        .bind(password_hash.as_str())
        .bind(created_at)
        .fetch_one(&self.pool)
        .await;
        let row = match result {
            Ok(row) => row,
            Err(error)
                if error
                    .as_database_error()
                    .is_some_and(DatabaseError::is_unique_violation) =>
            {
                return Err(UserError::DuplicateUsername);
            }
            Err(error) => return Err(storage_error(error)),
        };
        row.into_user()
    }

    async fn user_by_username(
        &self,
        username: &Username,
    ) -> Result<Option<UserCredentials>, UserError> {
        let row = sqlx::query_as::<_, UserCredentialsRow>(
            "SELECT id, username, password_hash, active, created_at
             FROM users WHERE username = ?",
        )
        .bind(username.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(UserCredentialsRow::into_credentials).transpose()
    }

    async fn user_by_id(&self, id: UserId) -> Result<Option<User>, UserError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, active, created_at FROM users WHERE id = ?",
        )
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(UserRow::into_user).transpose()
    }

    async fn list_users(&self) -> Result<Vec<User>, UserError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, active, created_at FROM users ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter().map(UserRow::into_user).collect()
    }

    async fn set_user_password(
        &self,
        id: UserId,
        password_hash: &PasswordHash,
    ) -> Result<(), UserError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let update = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(password_hash.as_str())
            .bind(id.get())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        if update.rows_affected() == 0 {
            return Err(UserError::NotFound);
        }
        revoke_user_sessions(&mut transaction, id)
            .await
            .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)
    }

    async fn set_user_active(&self, id: UserId, active: bool) -> Result<(), UserError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let update = sqlx::query("UPDATE users SET active = ? WHERE id = ?")
            .bind(if active { 1_i64 } else { 0_i64 })
            .bind(id.get())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        if update.rows_affected() == 0 {
            return Err(UserError::NotFound);
        }
        if !active {
            revoke_user_sessions(&mut transaction, id)
                .await
                .map_err(storage_error)?;
        }
        transaction.commit().await.map_err(storage_error)
    }

    async fn delete_user(&self, id: UserId) -> Result<(), UserError> {
        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id.get())
            .execute(&self.pool)
            .await
            .map_err(storage_error)?;
        if result.rows_affected() == 0 {
            return Err(UserError::NotFound);
        }
        Ok(())
    }
}

fn storage_error(source: sqlx::Error) -> UserError {
    UserError::Storage(Box::new(source))
}

#[derive(FromRow)]
struct UserRow {
    id: i64,
    username: String,
    active: i64,
    created_at: UserTimestamp,
}

impl UserRow {
    fn into_user(self) -> Result<User, UserError> {
        let id = UserId::new(self.id).map_err(|_| UserError::InvalidStoredUser)?;
        let username = Username::new(self.username).map_err(|_| UserError::InvalidStoredUser)?;
        let active = match self.active {
            0 => false,
            1 => true,
            _ => return Err(UserError::InvalidStoredUser),
        };
        Ok(User {
            id,
            username,
            active,
            created_at: self.created_at,
        })
    }
}

#[derive(FromRow)]
struct UserCredentialsRow {
    id: i64,
    username: String,
    password_hash: String,
    active: i64,
    created_at: UserTimestamp,
}

impl UserCredentialsRow {
    fn into_credentials(self) -> Result<UserCredentials, UserError> {
        let user = UserRow {
            id: self.id,
            username: self.username,
            active: self.active,
            created_at: self.created_at,
        }
        .into_user()?;
        let password_hash =
            PasswordHash::parse(&self.password_hash).map_err(|_| UserError::InvalidStoredUser)?;
        Ok(UserCredentials {
            user,
            password_hash,
        })
    }
}

#[cfg(test)]
#[path = "../../../unit-tests/identity/sqlite/users.rs"]
mod tests;
