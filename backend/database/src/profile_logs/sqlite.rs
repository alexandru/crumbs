use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;
use sqlx::FromRow;
use sqlx::QueryBuilder;
use sqlx::sqlite::Sqlite;

use crate::clock::UnixMillis;
use crate::foods::FoodDescription;
use crate::identity::UserId;
use crate::nutrition_model::{
    ConsumedNutrition, FoodData, Kilocalories, MeasuredQuantity, NutritionBasis, QuantityError,
    Reported, ServingSize,
};
use crate::profiles::{IanaTimezone, ProfileData, ProfileId};
use crate::runtime::Database;

use super::error::ProfileLogError;
use super::model::{
    BiometricEntry, CreateLog, FoodInputSnapshot, FoodLogEntry, FoodLogInput, FoodLogPatch,
    LocalTime, LogDay, LogEntryPatch, LogFilter, LogInput, LogPage, PatchLog, ProfileLog,
    ProfileLogData, ProfileLogId, ServingSelector,
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
        let Some(mut profile_data) = owned_profile_data(&mut transaction, user, profile).await?
        else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(None);
        };
        let now = self.clock.now().map_err(ProfileLogError::clock)?;
        let time = match input.time {
            Some(time) => time,
            None => local_time_at(profile_data.settings.timezone, now)?,
        };
        let data = resolve_log_input(&mut transaction, &input.entry).await?;
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
        if is_weight(&data) {
            update_current_weight_if_changed(
                &mut transaction,
                user,
                profile,
                &mut profile_data,
                now,
            )
            .await?;
        }
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
        let Some(mut profile_data) = owned_profile_data(&mut transaction, user, profile).await?
        else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(None);
        };
        let row = sqlx::query_as::<_, ProfileLogRow>(
            "SELECT id, profile_id, day, time, data_json
             FROM profile_logs WHERE profile_id = ? AND id = ?",
        )
        .bind(profile.get())
        .bind(id.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(ProfileLogError::storage)?;
        let Some(row) = row else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(None);
        };
        let current = row.into_log()?;
        let previous_weight = is_weight(&current.data);
        let day = match &patch.day {
            crate::profiles::Patch::Unchanged => current.day,
            crate::profiles::Patch::Set(day) => *day,
        };
        let time = match &patch.time {
            crate::profiles::Patch::Unchanged => current.time,
            crate::profiles::Patch::Set(time) => *time,
        };
        let replacement = match &patch.entry {
            crate::profiles::Patch::Unchanged => None,
            crate::profiles::Patch::Set(entry) => {
                replace_same_variant(&mut transaction, &current.data, entry).await?
            }
        };
        let data_changed = replacement
            .as_ref()
            .is_some_and(|data| data != &current.data);
        let changed = day != current.day || time != current.time || data_changed;
        if !changed {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(Some(current));
        }
        let data = replacement.unwrap_or(current.data);
        let now = self.clock.now().map_err(ProfileLogError::clock)?;
        sqlx::query(
            "UPDATE profile_logs
             SET day = ?, time = ?, data_json = ?, updated_at = ?
             WHERE profile_id = ? AND id = ?",
        )
        .bind(day.to_string())
        .bind(time.to_string())
        .bind(encode_data(&data)?)
        .bind(now)
        .bind(profile.get())
        .bind(id.get())
        .execute(&mut *transaction)
        .await
        .map_err(ProfileLogError::storage)?;
        if previous_weight || is_weight(&data) {
            update_current_weight_if_changed(
                &mut transaction,
                user,
                profile,
                &mut profile_data,
                now,
            )
            .await?;
        }
        transaction
            .commit()
            .await
            .map_err(ProfileLogError::storage)?;
        Ok(Some(ProfileLog {
            id,
            profile_id: profile,
            day,
            time,
            data,
        }))
    }

    async fn delete_profile_log(
        &self,
        user: UserId,
        profile: ProfileId,
        id: ProfileLogId,
    ) -> Result<bool, ProfileLogError> {
        let mut transaction = begin_immediate(self).await?;
        let Some(mut profile_data) = owned_profile_data(&mut transaction, user, profile).await?
        else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(false);
        };
        let row = sqlx::query_as::<_, ProfileLogRow>(
            "SELECT id, profile_id, day, time, data_json
             FROM profile_logs WHERE profile_id = ? AND id = ?",
        )
        .bind(profile.get())
        .bind(id.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(ProfileLogError::storage)?;
        let Some(row) = row else {
            transaction
                .commit()
                .await
                .map_err(ProfileLogError::storage)?;
            return Ok(false);
        };
        let log = row.into_log()?;
        sqlx::query("DELETE FROM profile_logs WHERE profile_id = ? AND id = ?")
            .bind(profile.get())
            .bind(id.get())
            .execute(&mut *transaction)
            .await
            .map_err(ProfileLogError::storage)?;
        if is_weight(&log.data) {
            let now = self.clock.now().map_err(ProfileLogError::clock)?;
            update_current_weight_if_changed(
                &mut transaction,
                user,
                profile,
                &mut profile_data,
                now,
            )
            .await?;
        }
        transaction
            .commit()
            .await
            .map_err(ProfileLogError::storage)?;
        Ok(true)
    }
}

async fn resolve_log_input(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    input: &LogInput,
) -> Result<ProfileLogData, ProfileLogError> {
    match input {
        LogInput::Biometric { measurement } => Ok(ProfileLogData::Biometric {
            measurement: *measurement,
        }),
        LogInput::Food(input) => Ok(ProfileLogData::Food(Box::new(
            resolve_food_input(transaction, input).await?,
        ))),
    }
}

async fn replace_same_variant(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    current: &ProfileLogData,
    patch: &LogEntryPatch,
) -> Result<Option<ProfileLogData>, ProfileLogError> {
    match (current, patch) {
        (ProfileLogData::Biometric { .. }, LogEntryPatch::Biometric { measurement }) => {
            Ok(Some(ProfileLogData::Biometric {
                measurement: *measurement,
            }))
        }
        (ProfileLogData::Food(current), LogEntryPatch::Food(patch)) => {
            if !patch.has_changes() {
                return Ok(None);
            }
            let input = patch_food_input(current, patch)?;
            Ok(Some(ProfileLogData::Food(Box::new(
                resolve_food_input(transaction, &input).await?,
            ))))
        }
        (ProfileLogData::Biometric { .. }, LogEntryPatch::Food(_))
        | (ProfileLogData::Food(_), LogEntryPatch::Biometric { .. }) => {
            Err(ProfileLogError::InvalidInput(
                "a log entry cannot change between food and biometric categories".to_owned(),
            ))
        }
    }
}

fn patch_food_input(
    current: &FoodLogEntry,
    patch: &FoodLogPatch,
) -> Result<FoodLogInput, ProfileLogError> {
    let food_id = match patch.food_id() {
        crate::profiles::Patch::Unchanged => current.food_id(),
        crate::profiles::Patch::Set(food_id) => Some(*food_id),
    }
    .ok_or_else(|| {
        ProfileLogError::InvalidInput("food_id is required to refresh this food entry".to_owned())
    })?;

    let amount = match patch.amount() {
        crate::profiles::Patch::Unchanged => current.input().map(FoodInputSnapshot::amount),
        crate::profiles::Patch::Set(amount) => Some(*amount),
    }
    .ok_or_else(|| {
        ProfileLogError::InvalidInput("amount is required to refresh this food entry".to_owned())
    })?;

    let serving_size = match patch.serving_size() {
        crate::profiles::Patch::Unchanged => current
            .input()
            .map(FoodInputSnapshot::serving_size)
            .map(|serving| ServingSelector::parse(&serving.name))
            .transpose()?,
        crate::profiles::Patch::Set(serving_size) => Some(serving_size.clone()),
    }
    .ok_or_else(|| {
        ProfileLogError::InvalidInput(
            "serving_size is required to refresh this food entry".to_owned(),
        )
    })?;

    FoodLogInput::new(food_id, amount, serving_size)
}

async fn resolve_food_input(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    input: &FoodLogInput,
) -> Result<FoodLogEntry, ProfileLogError> {
    let row = sqlx::query_as::<_, FoodSourceRow>(
        "SELECT description, parsed_data FROM crumbs_foods WHERE id = ?",
    )
    .bind(input.food_id().0)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(ProfileLogError::storage)?
    .ok_or_else(|| ProfileLogError::InvalidInput("food ID does not exist".to_owned()))?;
    let food_data = serde_json::from_str::<FoodData>(&row.parsed_data)
        .map_err(|_| ProfileLogError::InvalidStoredData)?;
    let nutrition = food_data.nutrition.as_ref().ok_or_else(missing_calories)?;
    let source_energy = nutrition.energy.ok_or_else(missing_calories)?;
    let serving = resolve_serving(&food_data, input.serving_size())?;
    let quantity = serving
        .quantity()
        .checked_scale(input.amount().value())
        .map_err(food_calculation_error)?;
    if quantity.value().is_zero() {
        return Err(unsupported_precision("consumed quantity"));
    }
    let factor = nutrition_factor(nutrition.per, quantity)?;
    let scaled_energy = source_energy
        .checked_scale(factor)
        .map_err(food_calculation_error)?;
    let calories = Reported::<Kilocalories>::new(
        scaled_energy.qualifier(),
        scaled_energy
            .amount()
            .to_kilocalories()
            .map_err(food_calculation_error)?,
    )
    .map_err(food_calculation_error)?;
    let nutrition = ConsumedNutrition {
        calories,
        nutrients: nutrition
            .nutrients
            .checked_scale(factor)
            .map_err(food_calculation_error)?,
    };
    let input_snapshot = FoodInputSnapshot::new(input.amount(), serving)?;
    FoodLogEntry::new(
        Some(input.food_id()),
        quantity,
        nutrition,
        row.description,
        Some(input_snapshot),
    )
}

fn resolve_serving(
    food: &FoodData,
    selector: &ServingSelector,
) -> Result<ServingSize, ProfileLogError> {
    match selector {
        ServingSelector::Unit(unit) => ServingSize::new(
            unit.as_str().to_owned(),
            unit.quantity().map_err(food_calculation_error)?,
        )
        .map_err(food_calculation_error),
        ServingSelector::Named(name) => {
            let mut matches = food
                .serving_sizes
                .iter()
                .filter(|serving| serving.name.trim() == name.as_str());
            let serving = matches.next().ok_or_else(|| {
                ProfileLogError::InvalidInput(format!(
                    "serving '{}' is not available for this food",
                    name.as_str()
                ))
            })?;
            if matches.next().is_some() {
                return Err(ProfileLogError::InvalidInput(format!(
                    "serving '{}' is ambiguous for this food",
                    name.as_str()
                )));
            }
            Ok(serving.clone())
        }
    }
}

fn nutrition_factor(
    basis: NutritionBasis,
    quantity: MeasuredQuantity,
) -> Result<Decimal, ProfileLogError> {
    let normalized = match (basis, quantity) {
        (NutritionBasis::Per100Grams, MeasuredQuantity::Mass(mass)) => {
            mass.to_grams().map_err(food_calculation_error)?.value()
        }
        (NutritionBasis::Per100Millilitres, MeasuredQuantity::Volume(volume)) => volume
            .to_millilitres()
            .map_err(food_calculation_error)?
            .value(),
        _ => {
            return Err(ProfileLogError::InvalidInput(
                "serving unit must match the food nutrition basis".to_owned(),
            ));
        }
    };
    if normalized.is_zero() {
        return Err(unsupported_precision("normalized quantity"));
    }
    let factor = normalized
        .checked_div(Decimal::from(100))
        .ok_or_else(|| unsupported_precision("nutrition scale"))?;
    if factor.is_zero() {
        return Err(unsupported_precision("nutrition scale"));
    }
    Ok(factor)
}

fn missing_calories() -> ProfileLogError {
    ProfileLogError::InvalidInput(
        "food has no usable calorie data; edit the food's nutrition or create a food with calories, then retry".to_owned(),
    )
}

fn unsupported_precision(value: &str) -> ProfileLogError {
    ProfileLogError::InvalidInput(format!(
        "{value} is below the supported fixed-point precision"
    ))
}

fn food_calculation_error(error: QuantityError) -> ProfileLogError {
    ProfileLogError::InvalidInput(format!(
        "food quantity or nutrition cannot be represented at supported fixed-point precision: {error}"
    ))
}

fn is_weight(data: &ProfileLogData) -> bool {
    matches!(
        data,
        ProfileLogData::Biometric {
            measurement: BiometricEntry::Weight(_)
        }
    )
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

async fn owned_profile_data(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    user: UserId,
    profile: ProfileId,
) -> Result<Option<ProfileData>, ProfileLogError> {
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
            serde_json::from_str::<ProfileData>(&json)
                .map_err(|_| ProfileLogError::InvalidStoredData)
        })
        .transpose()
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

async fn update_current_weight_if_changed(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    user: UserId,
    profile: ProfileId,
    data: &mut ProfileData,
    now: UnixMillis,
) -> Result<(), ProfileLogError> {
    let latest_json = sqlx::query_scalar::<_, String>(
        "SELECT data_json
         FROM profile_logs
         WHERE profile_id = ? AND entry_type = 'biometric' AND subtype = 'weight'
         ORDER BY day DESC, time DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(profile.get())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(ProfileLogError::storage)?;
    let latest = latest_json
        .map(|json| {
            let data = decode_data(&json)?;
            match data {
                ProfileLogData::Biometric {
                    measurement: BiometricEntry::Weight(measurement),
                } => Ok(measurement.value),
                ProfileLogData::Food(_) => Err(ProfileLogError::InvalidStoredData),
            }
        })
        .transpose()?;
    if data.cache.current_weight == latest {
        return Ok(());
    }
    data.cache.current_weight = latest;
    let data_json = serde_json::to_string(data).map_err(ProfileLogError::storage)?;
    let result = sqlx::query(
        "UPDATE profiles SET data_json = ?, updated_at = ?
         WHERE id = ? AND user_id = ?",
    )
    .bind(data_json)
    .bind(now)
    .bind(profile.get())
    .bind(user.get())
    .execute(&mut **transaction)
    .await
    .map_err(ProfileLogError::storage)?;
    if result.rows_affected() != 1 {
        return Err(ProfileLogError::InvalidStoredData);
    }
    Ok(())
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

#[derive(FromRow)]
struct FoodSourceRow {
    description: FoodDescription,
    parsed_data: String,
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
