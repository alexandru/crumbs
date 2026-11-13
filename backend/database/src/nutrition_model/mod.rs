//! Shared nutrition, serving, and measurement model.

mod model;
mod quantity;

pub use model::{
    Carbohydrates, ConsumedNutrition, Fat, FoodData, Minerals, Nutrients, NutritionBasis,
    NutritionProfile, ServingName, ServingSize, Sugars, Vitamins,
};
pub use quantity::{
    Centimetres, Energy, FixedUnit, Grams, Kcal, Kilograms, Kilojoules, Litres, Mass,
    MeasuredQuantity, Measurement, Micrograms, Milligrams, Millilitres, PerDay, PerWeek, Qualifier,
    QuantityError, Ratio, Reported, Volume,
};

pub(crate) use quantity::{KcalPerCm, KcalPerKg, parse_decimal};
