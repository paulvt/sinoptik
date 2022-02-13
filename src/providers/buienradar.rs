//! The Buienradar data provider.
//!
//! For more information about Buienradar, see: <https://www.buienradar.nl/overbuienradar/contact>
//! and <https://www.buienradar.nl/overbuienradar/gratis-weerdata>

use chrono::offset::TimeZone;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Local, NaiveDate, NaiveTime, Utc};
use chrono_tz::Europe;
use reqwest::Url;
use rocket::serde::Serialize;

use crate::Metric;

/// The base URL for the Buienradar API.
const BUIENRADAR_BASE_URL: &str = "https://gpsgadget.buienradar.nl/data/raintext";

/// The Buienradar API precipitation data item.
#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct Item {
    /// The time(stamp) of the forecast.
    #[serde(serialize_with = "ts_seconds::serialize")]
    time: DateTime<Utc>,

    /// The forecasted value.
    ///
    /// The unit is FIXME?
    value: f32,
}

/// Parses a line of the Buienradar precipitation text interface into an item.
///
// Each line has the format: `val|HH:MM`, for example: `362|12:30`.
fn parse_item(line: &str, today: &NaiveDate) -> Option<Item> {
    line.split_once('|')
        .map(|(v, t)| {
            let time = parse_time(t, today)?;
            let value = parse_value(v)?;

            Some(Item { time, value })
        })
        .flatten()
}

/// Parses a time string to date/time in the UTC time zone.
///
/// The provided time has the format `HH:MM` and is considered to be in the Europe/Amsterdam
/// time zone.
///
/// Returns [`None`] if the time cannot be parsed.
fn parse_time(t: &str, today: &NaiveDate) -> Option<DateTime<Utc>> {
    // First, get the naive date/time.
    let ntime = NaiveTime::parse_from_str(t, "%H:%M").ok()?;
    let ndtime = today.and_time(ntime);
    // Then, interpret the naive date/time in the Europe/Amsterdam time zone and convert it to the
    // UTC time zone.
    let ldtime = Europe::Amsterdam.from_local_datetime(&ndtime).unwrap();
    let dtime = ldtime.with_timezone(&Utc);

    Some(dtime)
}

/// Parses a precipitation value into an intensity value in mm/h.
///
/// For the conversion formula, see: <https://www.buienradar.nl/overbuienradar/gratis-weerdata>.
///
/// Returns [`None`] if the value cannot be parsed.
fn parse_value(v: &str) -> Option<f32> {
    let value = v.parse::<f32>().ok()?;
    let base: f32 = 10.0;
    let value = base.powf((value - 109.0) / 32.0);
    let value = (value * 10.0).round() / 10.0;

    Some(value)
}

/// Retrieves the Buienradar forecasted precipitation items for the provided position.
///
/// It only supports the following metric:
/// * [`Metric::Precipitation`]
///
/// Returns [`None`] if retrieval or deserialization fails, or if the metric is not supported by
/// this provider.
pub(crate) async fn get(lat: f64, lon: f64, metric: Metric) -> Option<Vec<Item>> {
    if metric != Metric::Precipitation {
        return None;
    }
    let mut url = Url::parse(BUIENRADAR_BASE_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("lat", &format!("{:.02}", lat))
        .append_pair("lon", &format!("{:.02}", lon));

    println!("▶️  Retrieving Buienradar data from {url}");
    let response = reqwest::get(url).await.ok()?;
    let output = match response.error_for_status() {
        Ok(res) => res.text().await.ok()?,
        Err(_err) => return None,
    };
    let today = Local::today().naive_utc();

    output
        .lines()
        .map(|line| parse_item(line, &today))
        .collect()
}
