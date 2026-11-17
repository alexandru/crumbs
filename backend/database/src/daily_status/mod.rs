mod error;
mod goals;
mod model;
mod sqlite;

pub use error::DailyStatusError;
pub use goals::{DailyGoals, GoalProgress, Percentage};
pub use model::{DailyExpenditure, DailyStatus, DailyStatusQuery, DailyStatusReport, IntakeTotal};

use std::future::Future;

use crate::identity::UserId;
use crate::profiles::ProfileId;

pub trait DailyStatusStore: Sync {
    fn daily_status(
        &self,
        user: UserId,
        profile: ProfileId,
        query: &DailyStatusQuery,
    ) -> impl Future<Output = Result<Option<DailyStatusReport>, DailyStatusError>> + Send;
}

#[cfg(test)]
#[path = "../../unit-tests/daily_status/mod.rs"]
mod tests;
