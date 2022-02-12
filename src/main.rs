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

use geocoding::{Forward, Openstreetmap, Point};
use rocket::serde::json::Json;
use rocket::serde::Serialize;
use rocket::{get, launch, routes, FromFormField};

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
    /// The air quality index (if asked for).
    #[serde(rename = "AQI")]
    aqi: Option<()>,
    /// The NO₂ concentration (if asked for).
    #[serde(rename = "NO2")]
    no2: Option<()>,
    #[serde(rename = "O3")]
    /// The O₃ concentration (if asked for).
    o3: Option<()>,
    #[serde(rename = "PAQI")]
    /// The FIXME air quality index (if asked for).
    paqi: Option<()>,
    #[serde(rename = "PM10")]
    /// The particulate matter in the air (if asked for).
    pm10: Option<()>,
    /// The pollen in the air (if asked for).
    pollen: Option<()>,
    /// The precipitation (if asked for).
    precipitation: Option<()>,
    /// The UV index (if asked for).
    #[serde(rename = "UVI")]
    uvi: Option<()>,
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
#[derive(Debug, Eq, PartialEq, FromFormField)]
enum Metric {
    /// All metrics.
    All,
    /// The air quality index.
    AQI,
    /// The NO₂ concentration.
    NO2,
    /// The O₃ concentration.
    O3,
    /// The FIXME air quality index.
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

/// Calculates and returns the forecast.
///
/// The provided list `metrics` determines what will be included in the forecast.
async fn forecast(lat: f64, lon: f64, metrics: Vec<Metric>) -> Forecast {
    let mut forecast = Forecast::new(lat, lon);

    for metric in metrics {
        match metric {
            // TODO: Find a way to handle the "All" case more gracefully!
            Metric::All => {
                forecast.aqi = Some(());
                forecast.no2 = Some(());
                forecast.o3 = Some(());
                forecast.paqi = Some(());
                forecast.pm10 = Some(());
                forecast.pollen = Some(());
                forecast.precipitation = Some(());
                forecast.uvi = Some(());
            }
            Metric::AQI => forecast.aqi = Some(()),
            Metric::NO2 => forecast.no2 = Some(()),
            Metric::O3 => forecast.o3 = Some(()),
            Metric::PAQI => forecast.paqi = Some(()),
            Metric::PM10 => forecast.pm10 = Some(()),
            Metric::Pollen => forecast.pollen = Some(()),
            Metric::Precipitation => forecast.precipitation = Some(()),
            Metric::UVI => forecast.uvi = Some(()),
        }
    }

    forecast
}

/// Retrieves the geocoded position for the given address.
async fn address_position(address: &str) -> Option<(f64, f64)> {
    let osm = Openstreetmap::new();
    // FIXME: Handle or log the error.
    let points: Vec<Point<f64>> = osm.forward(address).ok()?;

    points.get(0).map(|point| (point.x(), point.y()))
}

/// Handler for retrieving the forecast for an address.
#[get("/forecast?<address>&<metrics>")]
async fn forecast_address(address: String, metrics: Vec<Metric>) -> Option<Json<Forecast>> {
    let (lat, lon) = address_position(&address).await?;
    let forecast = forecast(lat, lon, metrics).await;

    Some(Json(forecast))
}

/// Handler for retrieving the forecast for a geocoded position.
#[get("/forecast?<lat>&<lon>&<metrics>", rank = 2)]
async fn forecast_geo(lat: f64, lon: f64, metrics: Vec<Metric>) -> Json<Forecast> {
    let forecast = forecast(lat, lon, metrics).await;

    Json(forecast)
}

/// Launches rocket.
#[launch]
async fn rocket() -> _ {
    rocket::build().mount("/", routes![forecast_address, forecast_geo])
}
