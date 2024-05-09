#![doc = include_str!("../README.md")]
#![warn(
    clippy::all,
    missing_copy_implementations,
    missing_debug_implementations,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links,
    trivial_casts,
    trivial_numeric_casts,
    renamed_and_removed_lints,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]
#![deny(missing_docs)]

use std::sync::{Arc, Mutex};

use rocket::fairing::AdHoc;
use rocket::http::Status;
use rocket::response::Responder;
use rocket::serde::json::Json;
use rocket::serde::Serialize;
use rocket::{get, routes, Build, Request, Rocket, State};

use self::forecast::{forecast, Forecast, Metric};
use self::maps::{mark_map, Error as MapsError, Maps, MapsHandle};
use self::position::{resolve_address, Position};

pub(crate) mod forecast;
pub(crate) mod maps;
pub(crate) mod position;
pub(crate) mod providers;

/// The possible provider errors that can occur.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// A CSV parse error occurred.
    #[error("CSV parse error: {0}")]
    CsvParse(#[from] csv::Error),

    /// A geocoding error occurred.
    #[error("Geocoding error: {0}")]
    Geocoding(#[from] geocoding::GeocodingError),

    /// An HTTP request error occurred.
    #[error("HTTP request error: {0}")]
    HttpRequest(#[from] reqwest::Error),

    /// Failed to join a task.
    #[error("Failed to join a task: {0}")]
    Join(#[from] rocket::tokio::task::JoinError),

    /// Failed to merge AQI & pollen items.
    #[error("Failed to merge AQI & pollen items: {0}")]
    Merge(#[from] providers::combined::MergeError),

    /// Failed to retrieve or sample the maps.
    #[error("Failed to retrieve or sample the maps: {0}")]
    Maps(#[from] maps::Error),

    /// No geocoded position could be found.
    #[error("No geocoded position could be found")]
    NoPositionFound,

    /// Encountered an unsupported metric.
    #[error("Encountered an unsupported metric: {0}")]
    UnsupportedMetric(Metric),
}

impl<'r, 'o: 'r> Responder<'r, 'o> for Error {
    fn respond_to(self, _request: &'r Request<'_>) -> rocket::response::Result<'o> {
        eprintln!("💥 Encountered error during request: {}", self);

        let status = match self {
            Error::NoPositionFound => Status::NotFound,
            Error::Maps(MapsError::NoMapsYet) => Status::ServiceUnavailable,
            Error::Maps(MapsError::OutOfBoundCoords(_, _)) => Status::NotFound,
            Error::Maps(MapsError::OutOfBoundOffset(_)) => Status::NotFound,
            _ => Status::InternalServerError,
        };

        Err(status)
    }
}

#[derive(Responder)]
#[response(content_type = "image/png")]
struct PngImageData(Vec<u8>);

/// Result type that defaults to [`Error`] as the default error type.
pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;

/// The version information as JSON response.
#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct VersionInfo {
    /// The version of the build.
    version: String,

    /// The timestamp of the build.
    timestamp: String,

    /// The (most recent) git SHA used for the build.
    git_sha: String,

    /// The timestamp of the last git commit used for the build.
    git_timestamp: String,
}
impl VersionInfo {
    /// Retrieves the version information from the environment variables.
    fn new() -> Self {
        Self {
            version: String::from(env!("CARGO_PKG_VERSION")),
            timestamp: String::from(env!("VERGEN_BUILD_TIMESTAMP")),
            git_sha: String::from(&env!("VERGEN_GIT_SHA")[0..7]),
            git_timestamp: String::from(env!("VERGEN_GIT_COMMIT_TIMESTAMP")),
        }
    }
}

/// Handler for retrieving the forecast for an address.
#[get("/forecast?<address>&<metrics>")]
async fn forecast_address(
    address: String,
    metrics: Vec<Metric>,
    maps_handle: &State<MapsHandle>,
) -> Result<Json<Forecast>> {
    let position = resolve_address(address).await?;
    let forecast = forecast(position, metrics, maps_handle).await;

    Ok(Json(forecast))
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
) -> Result<PngImageData> {
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
) -> Result<PngImageData> {
    let position = Position::new(lat, lon);
    let image_data = mark_map(position, metric, maps_handle).await;

    image_data.map(PngImageData)
}

/// Returns the version information.
#[get("/version", format = "application/json")]
async fn version() -> Result<Json<VersionInfo>> {
    Ok(Json(VersionInfo::new()))
}

/// Sets up Rocket.
fn rocket(maps_handle: MapsHandle) -> Rocket<Build> {
    let maps_refresher = maps::run(Arc::clone(&maps_handle));

    rocket::build()
        .mount(
            "/",
            routes![
                forecast_address,
                forecast_geo,
                map_address,
                map_geo,
                version
            ],
        )
        .manage(maps_handle)
        .attach(AdHoc::on_liftoff("Maps refresher", |_| {
            Box::pin(async move {
                // We don't care about the join handle nor error results?
                let _refresher = rocket::tokio::spawn(maps_refresher);
            })
        }))
        .attach(AdHoc::on_liftoff("Version", |_| {
            Box::pin(async move {
                let name = env!("CARGO_PKG_NAME");
                let version = env!("CARGO_PKG_VERSION");
                let git_sha = &env!("VERGEN_GIT_SHA")[0..7];

                println!("🌁 Started {name} v{version} (git @{git_sha})");
            })
        }))
}

/// Sets up Rocket and the maps cache refresher task.
pub fn setup() -> Rocket<Build> {
    let maps = Maps::new();
    let maps_handle = Arc::new(Mutex::new(maps));

    rocket(maps_handle)
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
        assert_float_absolute_eq!(json["lat"].as_f64().unwrap(), 51.448557, 1e-1);
        assert_float_absolute_eq!(json["lon"].as_f64().unwrap(), 5.450123, 1e-1);
        assert_matches!(json["time"], JsonValue::Number(_));
        assert_matches!(json.get("AQI"), None);
        assert_matches!(json.get("NO2"), None);
        assert_matches!(json.get("O3"), None);
        assert_matches!(json.get("PAQI"), None);
        assert_matches!(json.get("PM10"), None);
        assert_matches!(json.get("pollen"), None);
        assert_matches!(json.get("precipitation"), None);
        assert_matches!(json.get("UVI"), None);

        // Get a forecast with all metrics for the provided address.
        let response = client
            .get("/forecast?address=eindhoven&metrics=all")
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let json = response.into_json::<JsonValue>().expect("Not valid JSON");
        assert_float_absolute_eq!(json["lat"].as_f64().unwrap(), 51.448557, 1e-1);
        assert_float_absolute_eq!(json["lon"].as_f64().unwrap(), 5.450123, 1e-1);
        assert_matches!(json["time"], JsonValue::Number(_));
        assert_matches!(json.get("AQI"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("NO2"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("O3"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PAQI"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PM10"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("pollen"), Some(JsonValue::Array(_)));
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
        assert_matches!(json.get("NO2"), None);
        assert_matches!(json.get("O3"), None);
        assert_matches!(json.get("PAQI"), None);
        assert_matches!(json.get("PM10"), None);
        assert_matches!(json.get("pollen"), None);
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
        assert_matches!(json.get("NO2"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("O3"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PAQI"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("PM10"), Some(JsonValue::Array(_)));
        assert_matches!(json.get("pollen"), Some(JsonValue::Array(_)));
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
        assert_eq!(response.status(), Status::ServiceUnavailable);

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
        assert_eq!(response.status(), Status::UnprocessableEntity);
    }

    #[test]
    fn map_geo() {
        let maps_handle = Arc::new(Mutex::new(Maps::new()));
        let maps_handle_clone = Arc::clone(&maps_handle);
        let client = Client::tracked(rocket(maps_handle)).expect("Not a valid Rocket instance");

        // No maps available yet.
        let response = client.get("/map?lat=51.4&lon=5.5&metric=pollen").dispatch();
        assert_eq!(response.status(), Status::ServiceUnavailable);

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

        // ... but not if it is out of bounds.
        let response = client.get("/map?lat=0.0&lon=0.0&metric=pollen").dispatch();
        assert_eq!(response.status(), Status::NotFound);

        // No metric passed, don't know which map to show?
        let response = client.get("/map?lat=51.4&lon=5.5").dispatch();
        assert_eq!(response.status(), Status::UnprocessableEntity);
    }
}

