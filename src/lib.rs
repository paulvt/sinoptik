#![doc = include_str!("../README.md")]
#![warn(
    clippy::all,
    missing_debug_implementations,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links
)]
#![deny(missing_docs)]

use std::future::Future;
use std::sync::{Arc, Mutex};

use rocket::response::Responder;
use rocket::serde::json::Json;
use rocket::{get, routes, Build, Rocket, State};

pub(crate) use self::forecast::Metric;
use self::forecast::{forecast, Forecast};
pub(crate) use self::maps::{mark_map, Maps, MapsHandle};
use self::position::{resolve_address, Position};

pub(crate) mod forecast;
pub(crate) mod maps;
pub(crate) mod position;
pub(crate) mod providers;

#[derive(Responder)]
#[response(content_type = "image/png")]
struct PngImageData(Vec<u8>);

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
) -> Option<PngImageData> {
    let position = resolve_address(address).await?;
    let image_data = mark_map(position, metric, maps_handle).await;

    image_data.map(PngImageData)
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
) -> Option<PngImageData> {
    let position = Position::new(lat, lon);
    let image_data = mark_map(position, metric, maps_handle).await;

    image_data.map(PngImageData)
}

/// Sets up Rocket.
fn rocket(maps_handle: MapsHandle) -> Rocket<Build> {
    rocket::build().manage(maps_handle).mount(
        "/",
        routes![forecast_address, forecast_geo, map_address, map_geo],
    )
}

/// Sets up Rocket and the maps cache refresher task.
pub fn setup() -> (Rocket<Build>, impl Future<Output = ()>) {
    let maps = Maps::new();
    let maps_handle = Arc::new(Mutex::new(maps));
    let maps_refresher = maps::run(Arc::clone(&maps_handle));
    let rocket = rocket(maps_handle);

    (rocket, maps_refresher)
}

#[cfg(test)]
mod tests {
    use assert_float_eq::*;
    use assert_matches::assert_matches;
    use image::{DynamicImage, Rgba, RgbaImage};
    use rocket::http::{ContentType, Status};
    use rocket::local::blocking::Client;
    use rocket::serde::json::Value as JsonValue;

    use super::maps::RetrievedMaps;
    use super::*;

    fn maps_stub(map_count: u32) -> RetrievedMaps {
        let map_color = Rgba::from([73, 218, 33, 255]); // First color from map key.
        let image =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(820 * map_count, 988, map_color));

        RetrievedMaps::new(image)
    }

    fn maps_handle_stub() -> MapsHandle {
        let mut maps = Maps::new();
        maps.pollen = Some(maps_stub(24));
        maps.uvi = Some(maps_stub(5));

        Arc::new(Mutex::new(maps))
    }

    #[test]
    fn forecast_address() {
        let maps_handle = maps_handle_stub();
        let client = Client::tracked(rocket(maps_handle)).expect("Not a valid Rocket instance");

        // Get an empty forecast for the provided address.
        let response = client.get("/forecast?address=eindhoven").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let json = response.into_json::<JsonValue>().expect("Not valid JSON");
        assert_f64_near!(json["lat"].as_f64().unwrap(), 51.4392648);
        assert_f64_near!(json["lon"].as_f64().unwrap(), 5.478633);
        assert_matches!(json["time"], JsonValue::Number(_));
        assert_matches!(json.get("AQI"), None);
        assert_matches!(json.get("AQI_max"), None);
        assert_matches!(json.get("NO2"), None);
        assert_matches!(json.get("O3"), None);
        assert_matches!(json.get("PAQI"), None);
        assert_matches!(json.get("PM10"), None);
        assert_matches!(json.get("pollen"), None);
        assert_matches!(json.get("pollen_max"), None);
        assert_matches!(json.get("precipitation"), None);
        assert_matches!(json.get("UVI"), None);

        // Get a forecast with all metrics for the provided address.
        let response = client
            .get("/forecast?address=eindhoven&metrics=all")
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let json = response.into_json::<JsonValue>().expect("Not valid JSON");
        assert_f64_near!(json["lat"].as_f64().unwrap(), 51.4392648);
        assert_f64_near!(json["lon"].as_f64().unwrap(), 5.478633);
        assert_matches!(json["time"], JsonValue::Number(_));
        assert_matches!(json.get("AQI"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("AQI_max"), Some(JsonValue::Object(_)));
        assert_matches!(json.get("NO2"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("O3"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PAQI"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PM10"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("pollen"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("pollen_max"), Some(JsonValue::Object(_)));
        assert_matches!(json.get("precipitation"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("UVI"), Some(JsonValue::Array(_)));
    }

    #[test]
    fn forecast_geo() {
        let maps_handle = maps_handle_stub();
        let client = Client::tracked(rocket(maps_handle)).expect("valid Rocket instance");

        // Get an empty forecast for the geocoded location.
        let response = client.get("/forecast?lat=51.4&lon=5.5").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let json = response.into_json::<JsonValue>().expect("Not valid JSON");
        assert_f64_near!(json["lat"].as_f64().unwrap(), 51.4);
        assert_f64_near!(json["lon"].as_f64().unwrap(), 5.5);
        assert_matches!(json["time"], JsonValue::Number(_));
        assert_matches!(json.get("AQI"), None);
        assert_matches!(json.get("AQI_max"), None);
        assert_matches!(json.get("NO2"), None);
        assert_matches!(json.get("O3"), None);
        assert_matches!(json.get("PAQI"), None);
        assert_matches!(json.get("PM10"), None);
        assert_matches!(json.get("pollen"), None);
        assert_matches!(json.get("pollen_max"), None);
        assert_matches!(json.get("precipitation"), None);
        assert_matches!(json.get("UVI"), None);

        // Get a forecast with all metrics for the geocoded location.
        let response = client
            .get("/forecast?lat=51.4&lon=5.5&metrics=all")
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let json = response.into_json::<JsonValue>().expect("Not valid JSON");
        assert_f64_near!(json["lat"].as_f64().unwrap(), 51.4);
        assert_f64_near!(json["lon"].as_f64().unwrap(), 5.5);
        assert_matches!(json["time"], JsonValue::Number(_));
        assert_matches!(json.get("AQI"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("AQI_max"), Some(JsonValue::Object(_)));
        assert_matches!(json.get("NO2"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("O3"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PAQI"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PM10"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("pollen"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("pollen_max"), Some(JsonValue::Object(_)));
        assert_matches!(json.get("precipitation"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("UVI"), Some(JsonValue::Array(_)));
    }

    #[test]
    fn map_address() {
        let maps_handle = Arc::new(Mutex::new(Maps::new()));
        let maps_handle_clone = Arc::clone(&maps_handle);
        let client = Client::tracked(rocket(maps_handle)).expect("Not a valid Rocket instance");

        // No maps available yet.
        let response = client
            .get("/map?address=eindhoven&metric=pollen")
            .dispatch();
        assert_eq!(response.status(), Status::NotFound);

        // Load some dummy map.
        let mut maps = maps_handle_clone
            .lock()
            .expect("Maps handle mutex was poisoned");
        maps.pollen = Some(maps_stub(24));
        drop(maps);

        // There should be a map now.
        let response = client
            .get("/map?address=eindhoven&metric=pollen")
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.content_type(), Some(ContentType::PNG));

        // ... but not if it is out of bounds.
        let response = client.get("/map?address=berlin&metric=pollen").dispatch();
        assert_eq!(response.status(), Status::NotFound);

        // No metric selected, don't know which map to show?
        let response = client.get("/map?address=eindhoven").dispatch();
        assert_eq!(response.status(), Status::NotFound);
    }

    #[test]
    fn map_geo() {
        let maps_handle = Arc::new(Mutex::new(Maps::new()));
        let maps_handle_clone = Arc::clone(&maps_handle);
        let client = Client::tracked(rocket(maps_handle)).expect("Not a valid Rocket instance");

        // No metric passed, don't know which map to show?
        let response = client.get("/map?lat=51.4&lon=5.5").dispatch();
        assert_eq!(response.status(), Status::NotFound);

        // No maps available yet.
        let response = client.get("/map?lat=51.4&lon=5.5&metric=pollen").dispatch();
        assert_eq!(response.status(), Status::NotFound);

        // Load some dummy map.
        let mut maps = maps_handle_clone
            .lock()
            .expect("Maps handle mutex was poisoned");
        maps.pollen = Some(maps_stub(24));
        drop(maps);

        // There should be a map now.
        let response = client.get("/map?lat=51.4&lon=5.5&metric=pollen").dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.content_type(), Some(ContentType::PNG));

        // No metric passed, don't know which map to show?
        let response = client.get("/map?lat=51.4&lon=5.5").dispatch();
        assert_eq!(response.status(), Status::NotFound);
    }
}
