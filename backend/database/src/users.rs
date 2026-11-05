//! User identities and password credentials.

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;

use serde::Serialize;
use std::str::FromStr;

mod sqlite;

/// A positive local user identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct UserId(i64);

impl UserId {
    /// Creates a user identifier from a positive integer.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::InvalidUserId`] if `value` is not positive.
    pub const fn new(value: i64) -> Result<Self, UserError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(UserError::InvalidUserId)
        }
    }

    /// Returns the integer value of this identifier.
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A normalized account name.
///
/// Names contain 1–64 ASCII bytes. The first byte is alphanumeric. Remaining
/// bytes are alphanumeric, `_`, `.`, or `-`. Uppercase ASCII letters are
/// stored as lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Username(String);

impl Username {
    /// Validates and normalizes an account name.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::InvalidUsername`] if the name does not match the
    /// documented syntax.
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

    /// Returns the normalized account name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Timestamp stored as milliseconds since the Unix epoch.
pub type UserTimestamp = i64;

/// Public account metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: UserId,
    pub username: Username,
    pub active: bool,
    pub created_at: UserTimestamp,
}

/// A parsed bcrypt password hash.
///
/// The hash is never serialized and its debug representation is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(String);

impl PasswordHash {
    /// Hashes a password with bcrypt cost 12.
    ///
    /// Creation passwords must contain at least 6 Unicode scalar values and
    /// at most 72 UTF-8 bytes. Bcrypt input is not truncated.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::PasswordTooShort`] if the password has too few
    /// Unicode scalar values, [`UserError::PasswordTooLong`] if it exceeds 72
    /// UTF-8 bytes, or
    /// [`UserError::PasswordOperation`] if bcrypt cannot produce a hash.
    pub fn hash(password: &str) -> Result<Self, UserError> {
        validate_password_length(password, MIN_PASSWORD_CHARACTERS)?;
        let hash = bcrypt::non_truncating_hash(password, BCRYPT_COST)
            .map_err(|source| UserError::PasswordOperation(Box::new(source)))?;
        Ok(Self(hash))
    }

    /// Parses a bcrypt hash string.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::InvalidPasswordHash`] if the value is not a valid
    /// bcrypt hash.
    pub fn parse(value: &str) -> Result<Self, UserError> {
        bcrypt::HashParts::from_str(value).map_err(|_| UserError::InvalidPasswordHash)?;
        Ok(Self(value.to_owned()))
    }

    /// Checks a password against this hash without truncating the input.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::PasswordTooLong`] if the password exceeds 72 UTF-8
    /// bytes, or [`UserError::PasswordOperation`] if bcrypt cannot verify the
    /// hash.
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

/// A user record and its password hash for authentication.
///
/// This type does not implement serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCredentials {
    pub user: User,
    pub password_hash: PasswordHash,
}

/// Errors produced while validating or accessing user data.
#[derive(Debug, thiserror::Error)]
pub enum UserError {
    /// An account name does not match the supported syntax.
    #[error("invalid username")]
    InvalidUsername,
    /// A user ID is not positive.
    #[error("invalid user ID")]
    InvalidUserId,
    /// A password has fewer than 6 Unicode scalar values.
    #[error("password must be at least 6 characters")]
    PasswordTooShort,
    /// A password exceeds the bcrypt input limit of 72 UTF-8 bytes.
    #[error("password is too long")]
    PasswordTooLong,
    /// A stored password hash is not a valid bcrypt hash.
    #[error("invalid password hash")]
    InvalidPasswordHash,
    /// A normalized account name already exists.
    #[error("username already exists")]
    DuplicateUsername,
    /// The requested user does not exist.
    #[error("user not found")]
    NotFound,
    /// Stored user data does not satisfy the public model invariants.
    #[error("stored user data is invalid")]
    InvalidStoredUser,
    /// The user storage operation failed.
    #[error("user storage operation failed")]
    Storage(#[source] Box<dyn StdError + Send + Sync + 'static>),
    /// The application clock could not produce a valid timestamp.
    #[error("user timestamp could not be generated")]
    Clock(#[source] Box<dyn StdError + Send + Sync + 'static>),
    /// The password hash operation failed.
    #[error("password operation failed")]
    PasswordOperation(#[source] Box<dyn StdError + Send + Sync + 'static>),
}

/// Reads and manages users without exposing storage-engine types.
pub trait Users: Sync {
    /// Creates a user with an already-hashed password.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::DuplicateUsername`] if the normalized name exists,
    /// or a validation, clock, or storage error.
    fn create_user(
        &self,
        username: &Username,
        password_hash: &PasswordHash,
    ) -> impl Future<Output = Result<User, UserError>> + Send;

    /// Finds a user by normalized account name, including its password hash.
    ///
    /// Returns `None` if no user has the name.
    ///
    /// # Errors
    ///
    /// Returns a storage or stored-data error if the lookup fails.
    fn user_by_username(
        &self,
        username: &Username,
    ) -> impl Future<Output = Result<Option<UserCredentials>, UserError>> + Send;

    /// Finds public user metadata by identifier.
    ///
    /// Returns `None` if the identifier is absent.
    ///
    /// # Errors
    ///
    /// Returns a storage or stored-data error if the lookup fails.
    fn user_by_id(
        &self,
        id: UserId,
    ) -> impl Future<Output = Result<Option<User>, UserError>> + Send;

    /// Lists users in ascending identifier order.
    ///
    /// # Errors
    ///
    /// Returns a storage or stored-data error if the read fails.
    fn list_users(&self) -> impl Future<Output = Result<Vec<User>, UserError>> + Send;

    /// Replaces a password hash and revokes the user's sessions atomically.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::NotFound`] if the user is absent, or a storage error
    /// if the update or revocation fails.
    fn set_user_password(
        &self,
        id: UserId,
        password_hash: &PasswordHash,
    ) -> impl Future<Output = Result<(), UserError>> + Send;

    /// Sets whether the user can authenticate. Deactivation revokes sessions
    /// atomically with the status change.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::NotFound`] if the user is absent, or a storage error
    /// if the update or revocation fails.
    fn set_user_active(
        &self,
        id: UserId,
        active: bool,
    ) -> impl Future<Output = Result<(), UserError>> + Send;

    /// Deletes a user and relies on session foreign keys to cascade revocation.
    ///
    /// # Errors
    ///
    /// Returns [`UserError::NotFound`] if the user is absent, or a storage error
    /// if deletion fails.
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
#[path = "../unit-tests/users.rs"]
mod tests;
