//! The Buienradar data provider.
//!
//! For more information about Buienradar, see: <https://www.buienradar.nl/overbuienradar/contact>
//! and <https://www.buienradar.nl/overbuienradar/gratis-weerdata>.

use cached::proc_macro::cached;
use chrono::offset::TimeZone;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Local, NaiveTime, ParseError, Utc};
use chrono_tz::Europe;
use csv::ReaderBuilder;
use reqwest::Url;
use rocket::serde::{Deserialize, Serialize};

use crate::{cache_key, Metric};

/// The base URL for the Buienradar API.
const BUIENRADAR_BASE_URL: &str = "https://gpsgadget.buienradar.nl/data/raintext";

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
    time: DateTime<Utc>,

    /// The forecasted value.
    ///
    /// Its unit is mm/h.
    value: f32,
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
    // First, get the naive time.
    let ntime = NaiveTime::parse_from_str(t, "%H:%M")?;
    // FIXME: This might actually be the day before when started on a machine that
    //   doesn't run in the Europe/Amsterdam time zone.
    let ndtime = Local::today().naive_local().and_time(ntime);
    // Then, interpret the naive date/time in the Europe/Amsterdam time zone and convert it to the
    // UTC time zone.
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

/// Retrieves the Buienradar forecasted precipitation items for the provided position.
///
/// It only supports the following metric:
/// * [`Metric::Precipitation`]
///
/// Returns [`None`] if retrieval or deserialization fails, or if the metric is not supported by
/// this provider.
///
/// If the result is [`Some`] it will be cached for 5 minutes for the the given position and
/// metric.
#[cached(
    time = 300,
    convert = "{ cache_key(lat, lon, metric) }",
    key = "(i32, i32, Metric)",
    option = true
)]
pub(crate) async fn get(lat: f64, lon: f64, metric: Metric) -> Option<Vec<Item>> {
    if metric != Metric::Precipitation {
        return None;
    }
    let mut url = Url::parse(BUIENRADAR_BASE_URL).unwrap();
    url.query_pairs_mut()
        .append_pair("lat", &format!("{:.02}", lat))
        .append_pair("lon", &format!("{:.02}", lon));

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
    rdr.deserialize().collect::<Result<_, _>>().ok()
}
