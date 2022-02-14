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

use std::sync::{Arc, Mutex};

use cached::proc_macro::cached;
use color_eyre::Result;
use geocoding::{Forward, Openstreetmap, Point};
use rocket::serde::json::Json;
use rocket::serde::Serialize;
use rocket::tokio::{self, select};
use rocket::{get, routes, FromFormField, State};

use self::maps::{Maps, MapsHandle};
use self::providers::buienradar::Item as BuienradarItem;
use self::providers::luchtmeetnet::Item as LuchtmeetnetItem;

pub(crate) mod maps;
pub(crate) mod providers;

/// Caching key helper function that can be used by providers.
///
/// This is necessary because `f64` does not implement `Eq` nor `Hash`, which is required by
/// the caching implementation.
fn cache_key(lat: f64, lon: f64, metric: Metric) -> (i32, i32, Metric) {
    let lat_key = (lat * 10_000.0) as i32;
    let lon_key = (lon * 10_000.0) as i32;

    (lat_key, lon_key, metric)
}

/// The current for a specific location.
///
/// Only the metrics asked for are included as well as the position and current time.
///
/// TODO: Fill the metrics with actual data!
#[derive(Debug, Default, Serialize)]
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
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, FromFormField)]
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
async fn forecast(
    lat: f64,
    lon: f64,
    metrics: Vec<Metric>,
    _maps_handle: &State<MapsHandle>,
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

/// Retrieves the geocoded position for the given address.
///
/// Returns [`Some`] with tuple of latitude and longitude. Returns [`None`] if the address could
/// not be geocoded or the OpenStreetMap Nomatim API could not be contacted.
#[cached(size = 100)]
async fn address_position(address: String) -> Option<(f64, f64)> {
    println!("🌍 Geocoding the position of the address: {}", address);
    tokio::task::spawn_blocking(move || {
        let osm = Openstreetmap::new();
        let points: Vec<Point<f64>> = osm.forward(&address).ok()?;

        // The `geocoding` API always returns (longitude, latitude) as (x, y).
        points.get(0).map(|point| (point.y(), point.x()))
    })
    .await
    .ok()
    .flatten()
}

/// Handler for retrieving the forecast for an address.
#[get("/forecast?<address>&<metrics>")]
async fn forecast_address(
    address: String,
    metrics: Vec<Metric>,
    maps_handle: &State<MapsHandle>,
) -> Option<Json<Forecast>> {
    let (lat, lon) = address_position(address).await?;
    let forecast = forecast(lat, lon, metrics, maps_handle).await;

    Some(Json(forecast))
}

/// Handler for retrieving the forecast for a geocoded position.
#[get("/forecast?<lat>&<lon>&<metrics>", rank = 2)]
async fn forecast_geo(
    lat: f64,
    lon: f64,
    metrics: Vec<Metric>,
    maps_handle: &State<MapsHandle>,
) -> Json<Forecast> {
    let forecast = forecast(lat, lon, metrics, maps_handle).await;

    Json(forecast)
}

/// Starts the main maps refresh loop and sets up and launches Rocket.
///
/// See [`maps::run`] for the maps refresh loop.
#[rocket::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let maps = Maps::new();
    let maps_handle = Arc::new(Mutex::new(maps));
    let maps_updater = tokio::spawn(maps::run(Arc::clone(&maps_handle)));

    let rocket = rocket::build()
        .manage(maps_handle)
        .mount("/", routes![forecast_address, forecast_geo])
        .ignite()
        .await?;
    let shutdown = rocket.shutdown();

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
