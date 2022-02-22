#![doc = include_str!("../README.md")]
#![warn(
    clippy::all,
    missing_debug_implementations,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links
)]
#![deny(missing_docs)]

use std::sync::{Arc, Mutex};

use color_eyre::Result;
use rocket::http::ContentType;
use rocket::response::content::Custom;
use rocket::serde::json::Json;
use rocket::tokio::{self, select};
use rocket::{get, routes, State};

pub(crate) use self::forecast::Metric;
use self::forecast::{forecast, Forecast};
pub(crate) use self::maps::{mark_map, Maps, MapsHandle};
use self::position::{resolve_address, Position};

pub(crate) mod forecast;
pub(crate) mod maps;
pub(crate) mod position;
pub(crate) mod providers;

/// Handler for retrieving the forecast for an address.
#[get("/forecast?<address>&<metrics>")]
async fn forecast_address(
    address: String,
    metrics: Vec<Metric>,
    maps_handle: &State<MapsHandle>,
) -> Option<Json<Forecast>> {
    let position = resolve_address(address).await?;
    let forecast = forecast(position, metrics, maps_handle).await;

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
    let position = Position::new(lat, lon);
    let forecast = forecast(position, metrics, maps_handle).await;

    Json(forecast)
}

/// Handler for showing the current map with the geocoded position of an address for a specific
/// metric.
///
/// Note: This handler is mosly used for debugging purposes!
#[get("/map?<address>&<metric>")]
async fn map_address(
    address: String,
    metric: Metric,
    maps_handle: &State<MapsHandle>,
) -> Option<Custom<Vec<u8>>> {
    let position = resolve_address(address).await?;
    let image_data = mark_map(position, metric, maps_handle).await;

    image_data.map(|id| Custom(ContentType::PNG, id))
}

/// Handler for showing the current map with the geocoded position for a specific metric.
///
/// Note: This handler is mosly used for debugging purposes!
#[get("/map?<lat>&<lon>&<metric>", rank = 2)]
async fn map_geo(
    lat: f64,
    lon: f64,
    metric: Metric,
    maps_handle: &State<MapsHandle>,
) -> Option<Custom<Vec<u8>>> {
    let position = Position::new(lat, lon);
    let image_data = mark_map(position, metric, maps_handle).await;

    image_data.map(|id| Custom(ContentType::PNG, id))
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
        .mount(
            "/",
            routes![forecast_address, forecast_geo, map_address, map_geo],
        )
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
