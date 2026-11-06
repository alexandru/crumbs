use crate::identity::users::UserId;

pub(super) async fn revoke_user_sessions(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM users_sessions WHERE user_id = ?")
        .bind(user_id.get())
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM oauth_authorizations WHERE user_id = ?")
        .bind(user_id.get())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}
