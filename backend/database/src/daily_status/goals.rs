use rust_decimal::{Decimal, dec};
use serde::{Serialize, Serializer};

use crate::activity::{ActivityDuration, StepCount};
use crate::nutrition_model::{Kcal, Millilitres};
use crate::profiles::{ProfileDerived, ProfileSettings};

use super::DailyStatus;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DailyGoals {
    pub calories: Option<GoalProgress<Kcal>>,
    pub water: Option<GoalProgress<Millilitres>>,
    pub steps: Option<GoalProgress<StepCount>>,
    pub active_time: Option<GoalProgress<ActivityDuration>>,
}

impl DailyGoals {
    pub(super) fn calculate(
        status: &DailyStatus,
        settings: &ProfileSettings,
        derived: &ProfileDerived,
    ) -> Self {
        let calorie_target = derived
            .maintenance_calories()
            .zip(derived.calorie_balance_goal())
            .map(|(maintenance, balance)| (maintenance + balance).amount());

        Self {
            calories: calorie_target.map(|target| {
                GoalProgress::new(target, Some(status.calories_consumed), Kcal::value)
            }),
            water: settings.daily_water_goal.map(|target| {
                GoalProgress::new(
                    target.value().to_millilitres(),
                    Some(status.water),
                    Millilitres::value,
                )
            }),
            steps: settings.daily_step_goal.map(|target| {
                GoalProgress::new(target, status.steps, |steps| Decimal::from(*steps.value()))
            }),
            active_time: settings.daily_active_time_goal.map(|target| {
                GoalProgress::new(target, status.active_time, |duration| {
                    let duration = duration.value();
                    Decimal::from(duration.as_secs())
                        + Decimal::new(i64::from(duration.subsec_nanos()), 9)
                })
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GoalProgress<T> {
    pub target: T,
    pub percent: Option<Percentage>,
}

impl<T: Copy> GoalProgress<T> {
    fn new(target: T, total: Option<T>, value: impl Fn(T) -> Decimal) -> Self {
        let denominator = value(target);
        let percent = total
            .filter(|_| denominator > Decimal::ZERO)
            .map(|total| Percentage(value(total) / denominator * dec!(100)));

        Self { target, percent }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Percentage(Decimal);

impl Percentage {
    pub const fn value(self) -> Decimal {
        self.0
    }
}

impl Serialize for Percentage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0.normalize())
    }
}
