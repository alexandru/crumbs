use chrono::Datelike;
use futures_util::TryStreamExt;
use sqlx::FromRow;

use crate::identity::UserId;
use crate::nutrition_model::ConsumedNutrition;
use crate::profile_logs::{DailyActivityReading, ExerciseCalories, LogDay};
use crate::profiles::{ProfileData, ProfileDerived, ProfileId};
use crate::runtime::Database;

use super::{DailyStatus, DailyStatusError, DailyStatusQuery, DailyStatusReport, DailyStatusStore};

impl DailyStatusStore for Database {
    async fn daily_status(
        &self,
        user: UserId,
        profile: ProfileId,
        query: &DailyStatusQuery,
    ) -> Result<Option<DailyStatusReport>, DailyStatusError> {
        let mut transaction = self.pool.begin().await.map_err(DailyStatusError::storage)?;
        let data = sqlx::query_scalar::<_, String>(
            "SELECT data_json FROM profiles WHERE id = ? AND user_id = ?",
        )
        .bind(profile.get())
        .bind(user.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(DailyStatusError::storage)?;
        let Some(data) = data else {
            transaction
                .commit()
                .await
                .map_err(DailyStatusError::storage)?;
            return Ok(None);
        };

        let data: ProfileData =
            serde_json::from_str(&data).map_err(|_| DailyStatusError::InvalidStoredData)?;
        let now = self.clock.now().map_err(DailyStatusError::clock)?;
        let derived = ProfileDerived::calculate(&data.settings, &data.cache, now)?;
        let today = data.settings.timezone.local_day(now)?;
        let today = LogDay::new(today.year(), today.month(), today.day())
            .map_err(DailyStatusError::clock)?;
        let range = query.resolve(today)?;

        let mut rows = sqlx::query_as::<_, StatusRow>(
            "SELECT day, entry_type,
                    CASE entry_type
                        WHEN 'food' THEN json_extract(data_json, '$.nutrition')
                        WHEN 'exercise' THEN json_quote(json_extract(data_json, '$.calories_burned'))
                        WHEN 'daily_activity' THEN json_extract(data_json, '$.reading')
                    END AS payload
             FROM profile_logs
             WHERE profile_id = ? AND day >= ? AND day <= ?
                 AND entry_type IN ('food', 'exercise', 'daily_activity')
             ORDER BY day ASC, time DESC, id DESC",
        )
        .bind(profile.get())
        .bind(range.start().to_string())
        .bind(range.stop().to_string())
        .fetch(&mut *transaction);
        let mut next = rows.try_next().await.map_err(DailyStatusError::storage)?;
        let mut days = Vec::new();
        for day in range.days() {
            let mut status = DailyStatus::empty(day, &derived);
            let day_text = day.to_string();
            while let Some(row) = next {
                if row.day != day_text {
                    next = Some(row);
                    break;
                }

                row.add_to(&mut status)?;
                next = rows.try_next().await.map_err(DailyStatusError::storage)?;
            }

            status.finish(&data.settings, &derived);
            days.push(status);
        }
        if next.is_some() {
            return Err(DailyStatusError::InvalidStoredData);
        }
        drop(rows);

        transaction
            .commit()
            .await
            .map_err(DailyStatusError::storage)?;
        Ok(Some(DailyStatusReport {
            profile_id: profile,
            start: range.start(),
            stop: range.stop(),
            days,
        }))
    }
}

#[derive(FromRow)]
struct StatusRow {
    day: String,
    entry_type: String,
    payload: String,
}

impl StatusRow {
    fn add_to(self, status: &mut DailyStatus) -> Result<(), DailyStatusError> {
        match self.entry_type.as_str() {
            "food" => {
                let nutrition: ConsumedNutrition = serde_json::from_str(&self.payload)
                    .map_err(|_| DailyStatusError::InvalidStoredData)?;
                status.add_food(&nutrition);
            }
            "exercise" => {
                let calories: ExerciseCalories = serde_json::from_str(&self.payload)
                    .map_err(|_| DailyStatusError::InvalidStoredData)?;
                status.expenditure.exercise =
                    status.expenditure.exercise + *calories.value().value();
            }
            "daily_activity" => {
                let reading: DailyActivityReading = serde_json::from_str(&self.payload)
                    .map_err(|_| DailyStatusError::InvalidStoredData)?;
                match reading {
                    DailyActivityReading::Steps(steps) => {
                        status.steps.get_or_insert(steps);
                    }
                    DailyActivityReading::ActiveTime(duration) => {
                        status.active_time.get_or_insert(duration);
                    }
                }
            }
            _ => return Err(DailyStatusError::InvalidStoredData),
        }

        Ok(())
    }
}
