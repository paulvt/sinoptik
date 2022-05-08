//! The Luchtmeetnet open data provider.
//!
//! For more information about Luchtmeetnet, see: <https://www.luchtmeetnet.nl/contact>.

use cached::proc_macro::cached;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Duration, Utc};
use reqwest::Url;
use rocket::serde::{Deserialize, Serialize};

use crate::position::Position;
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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct Item {
    /// The time(stamp) of the forecast.
    #[serde(
        rename(deserialize = "timestamp_measured"),
        serialize_with = "ts_seconds::serialize"
    )]
    pub(crate) time: DateTime<Utc>,

    /// The forecasted value.
    ///
    /// The unit depends on the selected [metric](Metric).
    pub(crate) value: f32,
}

impl Item {
    #[cfg(test)]
    pub(crate) fn new(time: DateTime<Utc>, value: f32) -> Self {
        Self { time, value }
    }
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
///
/// If the result is [`Some`] it will be cached for 30 minutes for the the given position and
/// metric.
#[cached(time = 1800, option = true)]
pub(crate) async fn get(position: Position, metric: Metric) -> Option<Vec<Item>> {
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
        .append_pair("latitude", &position.lat_as_str(5))
        .append_pair("longitude", &position.lon_as_str(5));

    println!("▶️  Retrieving Luchtmeetnet data from: {url}");
    let response = reqwest::get(url).await.ok()?;
    let root: Container = match response.error_for_status() {
        Ok(res) => res.json().await.ok()?,
        Err(_err) => return None,
    };

    // Filter items that are older than one hour before now. They seem to occur sometimes?
    let too_old = Utc::now() - Duration::hours(1);
    let items = root
        .data
        .into_iter()
        .filter(|item| item.time > too_old)
        .collect();

    Some(items)
}
