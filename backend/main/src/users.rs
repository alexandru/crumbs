//! User creation for database commands.

use std::io;

use crumbs_database::Database;
use crumbs_database::DatabaseConfig;
use crumbs_database::users::{PasswordHash, User, UserError, Username, Users};
use zeroize::Zeroizing;

pub(super) fn prompt_credentials<P>(
    username: String,
    mut prompt: P,
) -> Result<(Username, PasswordHash), Error>
where
    P: FnMut(&str) -> io::Result<String>,
{
    let username = Username::new(username)?;
    let password = Zeroizing::new(
        prompt("Password (at least 6 characters): ").map_err(Error::PasswordPrompt)?,
    );
    let confirmation = Zeroizing::new(prompt("Confirm password: ").map_err(Error::PasswordPrompt)?);
    if password.as_str() != confirmation.as_str() {
        return Err(Error::PasswordMismatch);
    }

    let password_hash = PasswordHash::hash(password.as_str())?;
    Ok((username, password_hash))
}

pub(super) async fn create_user(
    config: DatabaseConfig,
    username: Username,
    password_hash: PasswordHash,
) -> Result<User, Error> {
    let database = Database::open(config).await?;
    let result = database.create_user(&username, &password_hash).await;
    database.close().await;
    result.map_err(Error::User)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to prompt for password: {0}")]
    PasswordPrompt(#[source] io::Error),
    #[error("passwords do not match")]
    PasswordMismatch,
    #[error(transparent)]
    Database(#[from] crumbs_database::Error),
    #[error(transparent)]
    User(#[from] UserError),
}

#[cfg(test)]
#[path = "../unit-tests/users.rs"]
mod tests;
