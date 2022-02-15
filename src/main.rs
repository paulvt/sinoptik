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

/// Handler for showing the current map with the geocoded position for a specific metric.
///
/// Note: This handler is mosly used for debugging purposes!
#[get("/map?<lat>&<lon>&<metric>")]
async fn show_map(
    lat: f64,
    lon: f64,
    metric: Metric,
    maps_handle: &State<MapsHandle>,
) -> Option<Custom<Vec<u8>>> {
    use image::{GenericImage, Rgba};
    use std::io::Cursor;

    let maps = maps_handle.lock().expect("Maps handle lock was poisoned");
    let mut image = match metric {
        Metric::PAQI => maps.pollen_first()?,
        Metric::Pollen => maps.pollen_first()?,
        Metric::UVI => maps.uvi_first()?,
        _ => return None, // Unsupported metric
    };

    // Paint the provided position on the map using a 11×11 pixel square.
    // FIXME: Use an actual correct calculation, instead of this very rough estimation!
    let rel_x = (lon - 3.3) / 4.0; // 3.3..7.3, 4.0
    let rel_y = 1.0 - ((lat - 50.7) / 3.0); // 50.7..53.7, 3.0
    let x = (rel_x * image.height() as f64) as u32;
    let y = (rel_y * image.width() as f64) as u32;
    for px in x - 5..=x + 5 {
        for py in y - 5..=y + 5 {
            image.put_pixel(px, py, Rgba::from([0xff, 0x0, 0x0, 0x70]));
        }
    }

    // Encode the image as PNG image data.
    // FIXME: This encoding call blocks the worker thread!
    let mut image_data = Cursor::new(Vec::new());
    image
        .write_to(
            &mut image_data,
            image::ImageOutputFormat::from(image::ImageFormat::Png),
        )
        .ok()?;

    Some(Custom(ContentType::PNG, image_data.into_inner()))
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
        .mount("/", routes![forecast_address, forecast_geo, show_map])
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
