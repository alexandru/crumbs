use sqlx::SqliteConnection;

use crate::identity::users::UserId;

pub(super) async fn revoke_user_sessions(
    connection: &mut SqliteConnection,
    user_id: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM users_sessions WHERE user_id = ?")
        .bind(user_id.get())
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM oauth_authorizations WHERE user_id = ?")
        .bind(user_id.get())
        .execute(&mut *connection)
        .await?;
    Ok(())
}
