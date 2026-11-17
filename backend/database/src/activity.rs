use std::fmt;
use std::time::Duration;

use crumbs_derive::{AliasNew, AliasValue};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AliasNew, AliasValue)]
#[serde(transparent)]
pub struct StepCount(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, AliasNew, AliasValue)]
pub struct ActivityDuration(Duration);

impl fmt::Display for ActivityDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds =
            Decimal::from(self.0.as_secs()) + Decimal::new(i64::from(self.0.subsec_nanos()), 9);
        write!(formatter, "{} s", seconds.normalize())
    }
}

impl Serialize for ActivityDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ActivityDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        humantime::parse_duration(&String::deserialize(deserializer)?)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}
