use std::ops::Add;

use serde::{Deserialize, Serialize};

use crate::activity::{ActivityDuration, StepCount};
use crate::nutrition_model::{
    ConsumedNutrition, Grams, Kcal, Measurement, Millilitres, Nutrients, Qualifier, Reported,
};
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
        let start = self.start.or(self.stop).unwrap_or(today);
        let stop = self.stop.unwrap_or(start);
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
    pub calories_consumed: IntakeTotal<Kcal>,
    pub nutrients: Nutrients<IntakeTotal<Grams>>,
    pub water: IntakeTotal<Millilitres>,
    pub steps: Option<StepCount>,
    pub active_time: Option<ActivityDuration>,
    pub expenditure: DailyExpenditure,
    pub goals: DailyGoals,
}

impl DailyStatus {
    pub(super) fn empty(day: LogDay, derived: &ProfileDerived) -> Self {
        let maintenance = derived.maintenance_calories().map(|value| value.amount());

        Self {
            day,
            calories_consumed: IntakeTotal::default(),
            nutrients: Nutrients::default(),
            water: IntakeTotal::default(),
            steps: None,
            active_time: None,
            expenditure: DailyExpenditure {
                maintenance,
                exercise: Kcal::ZERO,
                total: maintenance,
            },
            goals: DailyGoals::default(),
        }
    }

    pub(super) fn add_food(&mut self, nutrition: &ConsumedNutrition) {
        self.calories_consumed
            .add(Some(nutrition.calories), |amount| amount);
        self.nutrients
            .zip_mut(&nutrition.nutrients, |total, amount| {
                total.add(*amount, |mass| mass.to_grams());
            });
    }

    pub(super) fn finish(&mut self, settings: &ProfileSettings, derived: &ProfileDerived) {
        self.water = self.nutrients.water.water_volume_estimate();
        self.expenditure.total = self
            .expenditure
            .maintenance
            .map(|maintenance| maintenance + self.expenditure.exercise);
        self.goals = DailyGoals::calculate(self, settings, derived);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IntakeTotal<T> {
    pub total: T,
    pub complete: bool,
    pub exact: bool,
}

impl<T: Default> Default for IntakeTotal<T> {
    fn default() -> Self {
        Self {
            total: T::default(),
            complete: true,
            exact: true,
        }
    }
}

impl<T: Copy + Add<Output = T>> IntakeTotal<T> {
    fn add<U: Measurement>(&mut self, amount: Option<Reported<U>>, convert: impl FnOnce(U) -> T) {
        match amount {
            Some(amount) => {
                self.total = self.total + convert(amount.amount());
                self.exact &= amount.qualifier() == Qualifier::Exact;
            }
            None => self.complete = false,
        }
    }
}

impl IntakeTotal<Grams> {
    fn water_volume_estimate(self) -> IntakeTotal<Millilitres> {
        IntakeTotal {
            total: Millilitres::from(self.total.value()),
            complete: self.complete,
            exact: self.exact,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DailyExpenditure {
    pub maintenance: Option<Kcal>,
    pub exercise: Kcal,
    pub total: Option<Kcal>,
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
