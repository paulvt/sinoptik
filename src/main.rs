//! Service that provides today's weather forecast for air quality, rain and UV metrics.
//!
//! This is useful if you want to prepare for going outside and need to know what happens in the
//! near future or later today.

#![warn(
    clippy::all,
    missing_debug_implementations,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links
)]
#![deny(missing_docs)]

use color_eyre::Result;
use geocoding::{Forward, Openstreetmap, Point};
use rocket::serde::json::Json;
use rocket::serde::Serialize;
use rocket::tokio::{self, select};
use rocket::{get, routes, FromFormField};

use self::maps::Maps;

mod maps;

/// The current for a specific location.
///
/// Only the metrics asked for are included as well as the position and current time.
///
/// TODO: Fill the metrics with actual data!
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(crate = "rocket::serde")]
struct Forecast {
    /// The latitude of the position.
    lat: f64,

    /// The longitude of the position.
    lon: f64,

    /// The current time (in seconds since the UNIX epoch).
    time: i64,

    /// The air quality index (when asked for).
    #[serde(rename = "AQI", skip_serializing_if = "Option::is_none")]
    aqi: Option<u8>,

    /// The NO₂ concentration (when asked for).
    #[serde(rename = "NO2", skip_serializing_if = "Option::is_none")]
    no2: Option<u8>,

    /// The O₃ concentration (when asked for).
    #[serde(rename = "O3", skip_serializing_if = "Option::is_none")]
    o3: Option<u8>,

    /// The combination of pollen + air quality index (when asked for).
    #[serde(rename = "PAQI", skip_serializing_if = "Option::is_none")]
    paqi: Option<u8>,

    /// The particulate matter in the air (when asked for).
    #[serde(rename = "PM10", skip_serializing_if = "Option::is_none")]
    pm10: Option<u8>,

    /// The pollen in the air (when asked for).
    #[serde(skip_serializing_if = "Option::is_none")]
    pollen: Option<u8>,

    /// The precipitation (when asked for).
    #[serde(skip_serializing_if = "Option::is_none")]
    precipitation: Option<u8>,

    /// The UV index (when asked for).
    #[serde(rename = "UVI", skip_serializing_if = "Option::is_none")]
    uvi: Option<u8>,
}

impl Forecast {
    fn new(lat: f64, lon: f64) -> Self {
        let time = chrono::Utc::now().timestamp();

        Self {
            lat,
            lon,
            time,
            ..Default::default()
        }
    }
}

/// The supported metrics.
///
/// This is used for selecting which metrics should be calculated & returned.
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromFormField)]
enum Metric {
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
async fn forecast(lat: f64, lon: f64, metrics: Vec<Metric>) -> Forecast {
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
            Metric::All => unreachable!("should have been expanded"),
            Metric::AQI => forecast.aqi = Some(1),
            Metric::NO2 => forecast.no2 = Some(2),
            Metric::O3 => forecast.o3 = Some(3),
            Metric::PAQI => forecast.paqi = Some(4),
            Metric::PM10 => forecast.pm10 = Some(5),
            Metric::Pollen => forecast.pollen = Some(6),
            Metric::Precipitation => forecast.precipitation = Some(7),
            Metric::UVI => forecast.uvi = Some(8),
        }
    }

    forecast
}

/// Retrieves the geocoded position for the given address.
async fn address_position(address: String) -> Option<(f64, f64)> {
    tokio::task::spawn_blocking(move || {
        let osm = Openstreetmap::new();
        let points: Vec<Point<f64>> = osm.forward(&address).ok()?;

        points.get(0).map(|point| (point.x(), point.y()))
    })
    .await
    .ok()
    .flatten()
}

/// Handler for retrieving the forecast for an address.
#[get("/forecast?<address>&<metrics>")]
async fn forecast_address(address: String, metrics: Vec<Metric>) -> Option<Json<Forecast>> {
    let (lat, lon) = address_position(address).await?;
    let forecast = forecast(lat, lon, metrics).await;

    Some(Json(forecast))
}

/// Handler for retrieving the forecast for a geocoded position.
#[get("/forecast?<lat>&<lon>&<metrics>", rank = 2)]
async fn forecast_geo(lat: f64, lon: f64, metrics: Vec<Metric>) -> Json<Forecast> {
    let forecast = forecast(lat, lon, metrics).await;

    Json(forecast)
}

/// Starts the main maps refresh loop and sets up and launches Rocket.
#[rocket::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let rocket = rocket::build()
        .mount("/", routes![forecast_address, forecast_geo])
        .ignite()
        .await?;
    let shutdown = rocket.shutdown();

    let maps_updater = tokio::spawn(Maps::run());

    select! {
        result = rocket.launch() => {
            result?
        }
        result = maps_updater => {
            shutdown.notify();
            result?
        }
    }

    Ok(())
}
