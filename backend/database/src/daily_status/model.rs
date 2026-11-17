use serde::{Deserialize, Serialize};

use crate::activity::{ActivityDuration, StepCount};
use crate::nutrition_model::{ConsumedNutrition, Grams, Kcal, Millilitres, Nutrients};
use crate::profile_logs::LogDay;
use crate::profiles::{ProfileDerived, ProfileId, ProfileSettings};

use super::{DailyGoals, DailyStatusError};

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DailyStatusQuery {
    pub start: Option<LogDay>,
    pub stop: Option<LogDay>,
}

impl DailyStatusQuery {
    pub(super) fn resolve(self, today: LogDay) -> Result<DateRange, DailyStatusError> {
        let (start, stop) = match (self.start, self.stop) {
            (Some(start), Some(stop)) => (start, stop),
            (None, None) => (today, today),
            _ => {
                return Err(DailyStatusError::InvalidInput(
                    "start and stop must be provided together".to_owned(),
                ));
            }
        };
        if start > stop {
            return Err(DailyStatusError::InvalidInput(
                "start must be on or before stop".to_owned(),
            ));
        }

        Ok(DateRange { start, stop })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DailyStatusReport {
    pub profile_id: ProfileId,
    pub start: LogDay,
    pub stop: LogDay,
    pub days: Vec<DailyStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DailyStatus {
    pub day: LogDay,
    pub calories_consumed: Kcal,
    pub nutrients: Nutrients<Grams>,
    pub water: Millilitres,
    pub steps: Option<StepCount>,
    pub active_time: Option<ActivityDuration>,
    pub expenditure: DailyExpenditure,
    pub goals: DailyGoals,
}

impl DailyStatus {
    pub(super) fn calculate(
        day: LogDay,
        totals: DailyTotals,
        settings: &ProfileSettings,
        derived: &ProfileDerived,
    ) -> Self {
        let maintenance = derived.maintenance_calories().map(|value| value.amount());
        let water = water_volume_estimate(totals.nutrients.water);
        let goals = DailyGoals::calculate(&totals, water, settings, derived);

        Self {
            day,
            calories_consumed: totals.calories_consumed,
            nutrients: totals.nutrients,
            water,
            steps: totals.steps,
            active_time: totals.active_time,
            expenditure: DailyExpenditure {
                maintenance,
                exercise: totals.exercise,
                total: maintenance.map(|maintenance| maintenance + totals.exercise),
            },
            goals,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DailyExpenditure {
    pub maintenance: Option<Kcal>,
    pub exercise: Kcal,
    pub total: Option<Kcal>,
}

#[derive(Default)]
pub(super) struct DailyTotals {
    pub(super) calories_consumed: Kcal,
    pub(super) nutrients: Nutrients<Grams>,
    pub(super) steps: Option<StepCount>,
    pub(super) active_time: Option<ActivityDuration>,
    pub(super) exercise: Kcal,
}

impl DailyTotals {
    pub(super) fn add_food(&mut self, nutrition: &ConsumedNutrition) {
        self.calories_consumed = self.calories_consumed + nutrition.calories.amount();
        self.nutrients += &nutrition.nutrients;
    }
}

pub(super) struct DateRange {
    start: LogDay,
    stop: LogDay,
}

impl DateRange {
    pub(super) fn start(&self) -> LogDay {
        self.start
    }
    pub(super) fn stop(&self) -> LogDay {
        self.stop
    }
    pub(super) fn days(&self) -> impl Iterator<Item = LogDay> {
        self.start.through(self.stop)
    }
}

fn water_volume_estimate(water: Grams) -> Millilitres {
    Millilitres::from(water.value())
}
