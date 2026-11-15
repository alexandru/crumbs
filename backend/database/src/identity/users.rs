use std::error::Error as StdError;
use std::fmt;
use std::future::Future;

use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct UserId(i64);

impl UserId {
    pub const fn new(value: i64) -> Result<Self, UserError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(UserError::InvalidUserId)
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Username(String);

impl Username {
    pub fn new(value: impl AsRef<str>) -> Result<Self, UserError> {
        let value = value.as_ref();
        let mut bytes = value.bytes();
        let Some(first) = bytes.next() else {
            return Err(UserError::InvalidUsername);
        };
        if value.len() > MAX_USERNAME_BYTES
            || !first.is_ascii_alphanumeric()
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        {
            return Err(UserError::InvalidUsername);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub type UserTimestamp = i64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: UserId,
    pub username: Username,
    pub active: bool,
    pub created_at: UserTimestamp,
}

#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(String);

impl PasswordHash {
    pub fn hash(password: &str) -> Result<Self, UserError> {
        validate_password_length(password, MIN_PASSWORD_CHARACTERS)?;
        let hash = bcrypt::non_truncating_hash(password, BCRYPT_COST)
            .map_err(|source| UserError::PasswordOperation(Box::new(source)))?;
        Ok(Self(hash))
    }

    pub fn parse(value: &str) -> Result<Self, UserError> {
        bcrypt::HashParts::from_str(value).map_err(|_| UserError::InvalidPasswordHash)?;
        Ok(Self(value.to_owned()))
    }

    pub fn verify(&self, password: &str) -> Result<bool, UserError> {
        validate_password_length(password, 0)?;
        bcrypt::non_truncating_verify(password, &self.0)
            .map_err(|source| UserError::PasswordOperation(Box::new(source)))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PasswordHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasswordHash([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCredentials {
    pub user: User,
    pub password_hash: PasswordHash,
}

#[derive(Debug, thiserror::Error)]
pub enum UserError {
    #[error("invalid username")]
    InvalidUsername,
    #[error("invalid user ID")]
    InvalidUserId,
    #[error("password must be at least 6 characters")]
    PasswordTooShort,
    #[error("password is too long")]
    PasswordTooLong,
    #[error("invalid password hash")]
    InvalidPasswordHash,
    #[error("username already exists")]
    DuplicateUsername,
    #[error("user not found")]
    NotFound,
    #[error("stored user data is invalid")]
    InvalidStoredUser,
    #[error("user storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    #[error("user timestamp could not be generated")]
    Clock(#[source] Box<dyn StdError + Send + Sync + 'static>),
    #[error("password operation failed")]
    PasswordOperation(#[source] Box<dyn StdError + Send + Sync + 'static>),
}

pub trait Users: Sync {
    fn create_user(
        &self,
        username: &Username,
        password_hash: &PasswordHash,
    ) -> impl Future<Output = Result<User, UserError>> + Send;

    fn user_by_username(
        &self,
        username: &Username,
    ) -> impl Future<Output = Result<Option<UserCredentials>, UserError>> + Send;

    fn user_by_id(
        &self,
        id: UserId,
    ) -> impl Future<Output = Result<Option<User>, UserError>> + Send;

    fn list_users(&self) -> impl Future<Output = Result<Vec<User>, UserError>> + Send;

    fn set_user_password(
        &self,
        id: UserId,
        password_hash: &PasswordHash,
    ) -> impl Future<Output = Result<(), UserError>> + Send;

    fn set_user_active(
        &self,
        id: UserId,
        active: bool,
    ) -> impl Future<Output = Result<(), UserError>> + Send;

    fn delete_user(&self, id: UserId) -> impl Future<Output = Result<(), UserError>> + Send;
}

const BCRYPT_COST: u32 = 12;
const MAX_PASSWORD_BYTES: usize = 72;
const MIN_PASSWORD_CHARACTERS: usize = 6;
const MAX_USERNAME_BYTES: usize = 64;

fn validate_password_length(password: &str, minimum_characters: usize) -> Result<(), UserError> {
    if password.chars().count() < minimum_characters {
        return Err(UserError::PasswordTooShort);
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(UserError::PasswordTooLong);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../unit-tests/identity/users.rs"]
mod tests;
