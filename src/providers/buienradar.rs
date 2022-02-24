//! The Buienradar data provider.
//!
//! For more information about Buienradar, see: <https://www.buienradar.nl/overbuienradar/contact>
//! and <https://www.buienradar.nl/overbuienradar/gratis-weerdata>.

use cached::proc_macro::cached;
use chrono::offset::TimeZone;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Datelike, Duration, NaiveTime, ParseError, Utc};
use chrono_tz::Europe;
use csv::ReaderBuilder;
use reqwest::Url;
use rocket::serde::{Deserialize, Serialize};

use crate::maps::MapsHandle;
use crate::position::Position;
use crate::Metric;

/// The base URL for the Buienradar API.
const BUIENRADAR_BASE_URL: &str = "https://gpsgadget.buienradar.nl/data/raintext";

/// The Buienradar pollen/UV index map sample.
pub(crate) type Sample = crate::maps::Sample;

/// A row in the precipitation text output.
///
/// This is an intermediate type used to represent rows of the output.
#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Row {
    /// The precipitation value in the range `0..=255`.
    value: u16,

    /// The time in the `HH:MM` format.
    time: String,
}

/// The Buienradar API precipitation data item.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde", try_from = "Row")]
pub(crate) struct Item {
    /// The time(stamp) of the forecast.
    #[serde(serialize_with = "ts_seconds::serialize")]
    pub(crate) time: DateTime<Utc>,

    /// The forecasted value.
    ///
    /// Its unit is mm/h.
    pub(crate) value: f32,
}

impl TryFrom<Row> for Item {
    type Error = ParseError;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        let time = parse_time(&row.time)?;
        let value = convert_value(row.value);

        Ok(Item { time, value })
    }
}

/// Parses a time string to date/time in the UTC time zone.
///
/// The provided time has the format `HH:MM` and is considered to be in the Europe/Amsterdam
/// time zone.
fn parse_time(t: &str) -> Result<DateTime<Utc>, ParseError> {
    // First, get the current date in the Europe/Amsterdam time zone.
    let today = Utc::now().with_timezone(&Europe::Amsterdam).date();
    // Then, parse the time and interpret it relative to "today".
    let ntime = NaiveTime::parse_from_str(t, "%H:%M")?;
    let ndtime = today.naive_local().and_time(ntime);
    // Finally, interpret the naive date/time in the Europe/Amsterdam time zone and convert it to
    // the UTC time zone.
    let ldtime = Europe::Amsterdam.from_local_datetime(&ndtime).unwrap();
    let dtime = ldtime.with_timezone(&Utc);

    Ok(dtime)
}

/// Converts a precipitation value into an precipitation intensity value in mm/h.
///
/// For the conversion formula, see: <https://www.buienradar.nl/overbuienradar/gratis-weerdata>.
fn convert_value(v: u16) -> f32 {
    let base: f32 = 10.0;
    let value = base.powf((v as f32 - 109.0) / 32.0);

    (value * 10.0).round() / 10.0
}

/// Fix the timestamps of the items either before or after the day boundary.
///
/// If in the Europe/Amsterdam time zone it is still before 0:00, all timestamps after 0:00 need to
/// be bumped up with a day. If it is already after 0:00, all timestamps before 0:00 need to be
/// bumped back with a day.
// TODO: If something in Sinoptik needs unit tests, it is this!
fn fix_items_day_boundary(items: Vec<Item>) -> Vec<Item> {
    let now = Utc::now().with_timezone(&Europe::Amsterdam);
    // Use noon on the same day as "now" as a comparison moment.
    let noon = Europe::Amsterdam
        .ymd(now.year(), now.month(), now.day())
        .and_hms(12, 0, 0);

    if now < noon {
        // It is still before noon, so bump timestamps after noon a day back.
        items
            .into_iter()
            .map(|mut item| {
                if item.time > noon {
                    item.time = item.time - Duration::days(1)
                }
                item
            })
            .collect()
    } else {
        // It is already after noon, so bump the timestamps before noon a day forward.
        items
            .into_iter()
            .map(|mut item| {
                if item.time < noon {
                    item.time = item.time + Duration::days(1)
                }
                item
            })
            .collect()
    }
}

/// Retrieves the Buienradar forecasted precipitation items for the provided position.
///
/// Returns [`None`] if retrieval or deserialization fails.
///
/// If the result is [`Some`] it will be cached for 5 minutes for the the given position.
#[cached(time = 300, option = true)]
async fn get_precipitation(position: Position) -> Option<Vec<Item>> {
    let mut url = Url::parse(BUIENRADAR_BASE_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("lat", &position.lat_as_str(2))
        .append_pair("lon", &position.lon_as_str(2));

    println!("▶️  Retrieving Buienradar data from: {url}");
    let response = reqwest::get(url).await.ok()?;
    let output = match response.error_for_status() {
        Ok(res) => res.text().await.ok()?,
        Err(_err) => return None,
    };

    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .delimiter(b'|')
        .from_reader(output.as_bytes());
    let items: Vec<Item> = rdr.deserialize().collect::<Result<_, _>>().ok()?;

    // Check if the first item stamp is (timewise) later than the last item stamp.
    // In this case `parse_time` interpreted e.g. 23:00 and later 0:30 in the same day and some
    // time stamps need to be fixed.
    if items
        .first()
        .zip(items.last())
        .map(|(it1, it2)| it1.time > it2.time)
        == Some(true)
    {
        Some(fix_items_day_boundary(items))
    } else {
        Some(items)
    }
}

/// Retrieves the Buienradar forecasted pollen samples for the provided position.
///
/// Returns [`None`] if the sampling fails.
///
/// If the result is [`Some`] if will be cached for 1 hour for the given position.
#[cached(
    time = 3_600,
    key = "Position",
    convert = r#"{ position }"#,
    option = true
)]
async fn get_pollen(position: Position, maps_handle: &MapsHandle) -> Option<Vec<Sample>> {
    maps_handle
        .lock()
        .expect("Maps handle mutex was poisoned")
        .pollen_samples(position)
}

/// Retrieves the Buienradar forecasted UV index samples for the provided position.
///
/// Returns [`None`] if the sampling fails.
///
/// If the result is [`Some`] if will be cached for 1 day for the given position.
#[cached(
    time = 86_400,
    key = "Position",
    convert = r#"{ position }"#,
    option = true
)]
async fn get_uvi(position: Position, maps_handle: &MapsHandle) -> Option<Vec<Sample>> {
    maps_handle
        .lock()
        .expect("Maps handle mutex was poisoned")
        .uvi_samples(position)
}

/// Retrieves the Buienradar forecasted map samples for the provided position.
///
/// It only supports the following metric:
/// * [`Metric::Pollen`]
/// * [`Metric::UVI`]
///
/// Returns [`None`] if retrieval or deserialization fails, or if the metric is not supported by
/// this provider.
pub(crate) async fn get_samples(
    position: Position,
    metric: Metric,
    maps_handle: &MapsHandle,
) -> Option<Vec<Sample>> {
    match metric {
        Metric::Pollen => get_pollen(position, maps_handle).await,
        Metric::UVI => get_uvi(position, maps_handle).await,
        _ => None,
    }
}

/// Retrieves the Buienradar forecasted items for the provided position.
///
/// It only supports the following metric:
/// * [`Metric::Precipitation`]
///
/// Returns [`None`] if retrieval or deserialization fails, or if the metric is not supported by
/// this provider.
pub(crate) async fn get_items(position: Position, metric: Metric) -> Option<Vec<Item>> {
    match metric {
        Metric::Precipitation => get_precipitation(position).await,
        _ => None,
    }
}
