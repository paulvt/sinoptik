#![doc = include_str!("../README.md")]
#![warn(
    clippy::all,
    missing_debug_implementations,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links
)]
#![deny(missing_docs)]

use std::sync::{Arc, Mutex};

use chrono::Utc;
use color_eyre::Result;
use rocket::http::ContentType;
use rocket::response::content::Custom;
use rocket::serde::json::Json;
use rocket::tokio::{self, select};
use rocket::{get, routes, State};

pub(crate) use self::forecast::Metric;
use self::forecast::{forecast, Forecast};
pub(crate) use self::maps::{Maps, MapsHandle};
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
async fn show_map_address(
    address: String,
    metric: Metric,
    maps_handle: &State<MapsHandle>,
) -> Option<Custom<Vec<u8>>> {
    let position = resolve_address(address).await?;
    let image_data = draw_position(position, metric, maps_handle).await;

    image_data.map(|id| Custom(ContentType::PNG, id))
}

/// Handler for showing the current map with the geocoded position for a specific metric.
///
/// Note: This handler is mosly used for debugging purposes!
#[get("/map?<lat>&<lon>&<metric>", rank = 2)]
async fn show_map_geo(
    lat: f64,
    lon: f64,
    metric: Metric,
    maps_handle: &State<MapsHandle>,
) -> Option<Custom<Vec<u8>>> {
    let position = Position::new(lat, lon);
    let image_data = draw_position(position, metric, maps_handle).await;

    image_data.map(|id| Custom(ContentType::PNG, id))
}

/// Draws a crosshair on a map for the given position.
///
/// The map that is used is determined by the metric.
// FIXME: Maybe move this to the `maps` module?
async fn draw_position(
    position: Position,
    metric: Metric,
    maps_handle: &MapsHandle,
) -> Option<Vec<u8>> {
    use image::{GenericImage, Rgba};
    use std::io::Cursor;

    let maps_handle = Arc::clone(maps_handle);
    tokio::task::spawn_blocking(move || {
        let now = Utc::now();
        let maps = maps_handle.lock().expect("Maps handle lock was poisoned");
        let (mut image, coords) = match metric {
            Metric::PAQI => (maps.pollen_at(now)?, maps.pollen_project(position)),
            Metric::Pollen => (maps.pollen_at(now)?, maps.pollen_project(position)),
            Metric::UVI => (maps.uvi_at(now)?, maps.uvi_project(position)),
            _ => return None, // Unsupported metric
        };
        drop(maps);

        if let Some((x, y)) = coords {
            for py in 0..(image.height() - 1) {
                image.put_pixel(x, py, Rgba::from([0x00, 0x00, 0x00, 0x70]));
            }

            for px in 0..(image.width() - 1) {
                image.put_pixel(px, y, Rgba::from([0x00, 0x00, 0x00, 0x70]));
            }
        }

        // Encode the image as PNG image data.
        let mut image_data = Cursor::new(Vec::new());
        image
            .write_to(
                &mut image_data,
                image::ImageOutputFormat::from(image::ImageFormat::Png),
            )
            .ok()?;

        Some(image_data.into_inner())
    })
    .await
    .ok()
    .flatten()
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
            routes![
                forecast_address,
                forecast_geo,
                show_map_address,
                show_map_geo
            ],
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
