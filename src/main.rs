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
use rocket::tokio::{self, select};
use rocket::{get, routes, State};

pub(crate) use self::forecast::{forecast, Forecast, Metric};
pub(crate) use self::maps::{Maps, MapsHandle};

pub(crate) mod forecast;
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

/// Retrieves the geocoded position for the given address.
///
/// Returns [`Some`] with tuple of latitude and longitude. Returns [`None`] if the address could
/// not be geocoded or the OpenStreetMap Nomatim API could not be contacted.
///
/// If the result is [`Some`] it will be cached. Only the 100 least-recently used address
/// will be cached.
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
