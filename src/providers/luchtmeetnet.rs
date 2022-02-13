//! The Luchtmeetnet open data provider.
//!
//! For more information about Luchtmeetnet, see: <https://www.luchtmeetnet.nl/contact>.

use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use rocket::serde::{Deserialize, Serialize};

use crate::Metric;

/// The base URL for the Luchtmeetnet API.
const LUCHTMEETNET_BASE_URL: &str = "https://api.luchtmeetnet.nl/open_api/concentrations";

/// The Luchtmeetnet API data container.
///
/// This is only used temporarily during deserialization.
#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Container {
    data: Vec<Item>,
}

/// The Luchtmeetnet API data item.
#[derive(Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct Item {
    /// The time for when the value is forecast.
    #[serde(
        rename(deserialize = "timestamp_measured"),
        serialize_with = "ts_seconds::serialize"
    )]
    time: DateTime<Utc>,
    /// The forecasted value.
    ///
    /// The unit depends on the selected [metric](Metric).
    value: f32,
}

/// Retrieves the Luchtmeetnet forecasted items for the provided position and metric.
///
/// Returns [`None`] if retrieval or deserialization fails, or if the metric is not supported by
/// this provider.
pub(crate) async fn get(lat: f64, lon: f64, metric: Metric) -> Option<Vec<Item>> {
    let formula = match metric {
        Metric::AQI => "lki",
        Metric::NO2 => "no2",
        Metric::O3 => "o3",
        Metric::PM10 => "pm10",
        _ => return None, // Unsupported metric
    };
    let url = format!(
        "{LUCHTMEETNET_BASE_URL}?formula={formula}&latitude={:.05}&longitude={:.05}",
        lat, lon
    );

    println!("▶️  Retrieving Luchtmeetnet data from {url}");
    let response = reqwest::get(&url).await.ok()?;
    let root: Container = match response.error_for_status() {
        Ok(res) => res.json().await.ok()?,
        Err(_err) => return None,
    };

    Some(root.data)
}
