use std::error::Error as StdError;
use std::io;

use super::*;
use crate::DatabaseConfig;
use crate::runtime::Database;

#[tokio::test]
async fn user_management_persists_normalized_names_and_credentials() -> Result<(), Box<dyn StdError>>
{
    let (_directory, database) = open_database().await?;
    let password_hash = PasswordHash::hash("correct horse battery staple")?;
    let username = Username::new("Alice")?;
    let user = database.create_user(&username, &password_hash).await?;

    assert_eq!(user.username.as_str(), "alice");
    assert!(user.active);
    assert_eq!(database.user_by_id(user.id).await?, Some(user.clone()));
    assert_eq!(database.list_users().await?, vec![user.clone()]);

    let equivalent_name = Username::new("ALICE")?;
    let credentials = database
        .user_by_username(&equivalent_name)
        .await?
        .ok_or_else(|| io::Error::other("created user was not found by username"))?;
    assert_eq!(credentials.user, user);
    assert!(
        credentials
            .password_hash
            .verify("correct horse battery staple")?
    );
    assert!(matches!(
        database.create_user(&equivalent_name, &password_hash).await,
        Err(UserError::DuplicateUsername)
    ));

    database.set_user_password(user.id, &password_hash).await?;
    database.set_user_active(user.id, false).await?;
    let deactivated = database
        .user_by_id(user.id)
        .await?
        .ok_or_else(|| io::Error::other("deactivated user was not found"))?;
    assert!(!deactivated.active);

    let missing_id = UserId::new(user.id.get() + 100)?;
    assert_eq!(database.user_by_id(missing_id).await?, None);
    assert!(matches!(
        database.set_user_password(missing_id, &password_hash).await,
        Err(UserError::NotFound)
    ));

    database.delete_user(user.id).await?;
    assert_eq!(database.user_by_id(user.id).await?, None);
    assert!(matches!(
        database.delete_user(user.id).await,
        Err(UserError::NotFound)
    ));
    database.close().await;
    Ok(())
}

async fn open_database() -> Result<(tempfile::TempDir, Database), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;
    Ok((directory, database))
}
