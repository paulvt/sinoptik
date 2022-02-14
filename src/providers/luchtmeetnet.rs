//! The Luchtmeetnet open data provider.
//!
//! For more information about Luchtmeetnet, see: <https://www.luchtmeetnet.nl/contact>.

use cached::proc_macro::cached;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use reqwest::Url;
use rocket::serde::{Deserialize, Serialize};

use crate::{cache_key, Metric};

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
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct Item {
    /// The time(stamp) of the forecast.
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
/// It supports the following metrics:
/// * [`Metric::AQI`]
/// * [`Metric::NO2`]
/// * [`Metric::O3`]
/// * [`Metric::PM10`]
///
/// Returns [`None`] if retrieval or deserialization fails, or if the metric is not supported by
/// this provider.
#[cached(
    time = 300,
    convert = "{ cache_key(lat, lon, metric) }",
    key = "(i32, i32, Metric)",
    option = true
)]
pub(crate) async fn get(lat: f64, lon: f64, metric: Metric) -> Option<Vec<Item>> {
    let formula = match metric {
        Metric::AQI => "lki",
        Metric::NO2 => "no2",
        Metric::O3 => "o3",
        Metric::PM10 => "pm10",
        _ => return None, // Unsupported metric
    };
    let mut url = Url::parse(LUCHTMEETNET_BASE_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("formula", formula)
        .append_pair("latitude", &format!("{:.05}", lat))
        .append_pair("longitude", &format!("{:.05}", lon));

    println!("▶️  Retrieving Luchtmeetnet data from: {url}");
    let response = reqwest::get(url).await.ok()?;
    let root: Container = match response.error_for_status() {
        Ok(res) => res.json().await.ok()?,
        Err(_err) => return None,
    };

    Some(root.data)
}
