//! Forecast retrieval and construction.
//!
//! This module is used to construct a [`Forecast`] for the given position by retrieving data for
//! the requested metrics from their providers.

use rocket::serde::Serialize;

use crate::maps::MapsHandle;
use crate::providers;
use crate::providers::buienradar::Item as BuienradarItem;
use crate::providers::luchtmeetnet::Item as LuchtmeetnetItem;

/// The current forecast for a specific location.
///
/// Only the metrics asked for are included as well as the position and current time.
///
// TODO: Fill in missing data (#3)
#[derive(Debug, Default, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct Forecast {
    /// The latitude of the position.
    lat: f64,

    /// The longitude of the position.
    lon: f64,

    /// The current time (in seconds since the UNIX epoch).
    time: i64,

    /// The air quality index (when asked for).
    #[serde(rename = "AQI", skip_serializing_if = "Option::is_none")]
    aqi: Option<Vec<LuchtmeetnetItem>>,

    /// The NO₂ concentration (when asked for).
    #[serde(rename = "NO2", skip_serializing_if = "Option::is_none")]
    no2: Option<Vec<LuchtmeetnetItem>>,

    /// The O₃ concentration (when asked for).
    #[serde(rename = "O3", skip_serializing_if = "Option::is_none")]
    o3: Option<Vec<LuchtmeetnetItem>>,

    /// The combination of pollen + air quality index (when asked for).
    #[serde(rename = "PAQI", skip_serializing_if = "Option::is_none")]
    paqi: Option<()>,

    /// The particulate matter in the air (when asked for).
    #[serde(rename = "PM10", skip_serializing_if = "Option::is_none")]
    pm10: Option<Vec<LuchtmeetnetItem>>,

    /// The pollen in the air (when asked for).
    #[serde(skip_serializing_if = "Option::is_none")]
    pollen: Option<()>,

    /// The precipitation (when asked for).
    #[serde(skip_serializing_if = "Option::is_none")]
    precipitation: Option<Vec<BuienradarItem>>,

    /// The UV index (when asked for).
    #[serde(rename = "UVI", skip_serializing_if = "Option::is_none")]
    uvi: Option<()>,
}

impl Forecast {
    fn new(position: Position) -> Self {
        Self {
            lat: position.lat,
            lon: position.lon,
            time: chrono::Utc::now().timestamp(),

            ..Default::default()
        }
    }
}

/// The supported forecast metrics.
///
/// This is used for selecting which metrics should be calculated & returned.
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, rocket::FromFormField)]
pub(crate) enum Metric {
    /// All metrics.
    #[field(value = "all")]
    All,
    /// The air quality index.
    AQI,
    /// The NO₂ concentration.
    NO2,
    /// The O₃ concentration.
    O3,
    /// The combination of pollen + air quality index.
    PAQI,
    /// The particulate matter in the air.
    PM10,
    /// The pollen in the air.
    Pollen,
    /// The precipitation.
    Precipitation,
    /// The UV index.
    UVI,
}

impl Metric {
    /// Returns all supported metrics.
    fn all() -> Vec<Metric> {
        use Metric::*;

        Vec::from([AQI, NO2, O3, PAQI, PM10, Pollen, Precipitation, UVI])
    }
}

/// Calculates and returns the forecast.
///
/// The provided list `metrics` determines what will be included in the forecast.
pub(crate) async fn forecast(
    lat: f64,
    lon: f64,
    metrics: Vec<Metric>,
    _maps_handle: &MapsHandle,
) -> Forecast {
    let mut forecast = Forecast::new(lat, lon);

    // Expand the `All` metric if present, deduplicate otherwise.
    let mut metrics = metrics;
    if metrics.contains(&Metric::All) {
        metrics = Metric::all();
    } else {
        metrics.dedup()
    }

    for metric in metrics {
        match metric {
            // This should have been expanded to all the metrics matched below.
            Metric::All => unreachable!("The all metric should have been expanded"),
            Metric::AQI => forecast.aqi = providers::luchtmeetnet::get(lat, lon, metric).await,
            Metric::NO2 => forecast.no2 = providers::luchtmeetnet::get(lat, lon, metric).await,
            Metric::O3 => forecast.o3 = providers::luchtmeetnet::get(lat, lon, metric).await,
            Metric::PAQI => forecast.paqi = Some(()),
            Metric::PM10 => forecast.pm10 = providers::luchtmeetnet::get(lat, lon, metric).await,
            Metric::Pollen => forecast.pollen = Some(()),
            Metric::Precipitation => {
                forecast.precipitation = providers::buienradar::get(lat, lon, metric).await
            }
            Metric::UVI => forecast.uvi = Some(()),
        }
    }

    forecast
}
