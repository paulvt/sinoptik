//! The combined data provider.
//!
//! This combines and collates data using the other providers.

use std::time::Duration;

use cached::proc_macro::cached;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use rocket::serde::Serialize;

pub(crate) use super::buienradar::{self, Sample as BuienradarSample};
pub(crate) use super::luchtmeetnet::{self, Item as LuchtmeetnetItem};
use crate::maps::MapsHandle;
use crate::position::Position;
use crate::{Error, Metric};

/// The possible merge errors that can occur.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, thiserror::Error, PartialEq)]
pub(crate) enum MergeError {
    /// No AQI item found.
    #[error("No AQI item found")]
    NoAqiItemFound,

    /// No pollen item found.
    #[error("No pollen item found")]
    NoPollenItemFound,

    /// No AQI item found within 30 minutes of first pollen item.
    #[error("No AQI item found within 30 minutes of first pollen item")]
    NoCloseAqiItemFound,

    /// No pollen item found within 30 minutes of first AQI item.
    #[error("No pollen item found within 30 minutes of first AQI item")]
    NoClosePollenItemFound,
}

/// The combined data item.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct Item {
    /// The time(stamp) of the forecast.
    #[serde(serialize_with = "ts_seconds::serialize")]
    time: DateTime<Utc>,

    /// The forecasted value.
    value: f32,
}

impl Item {
    #[cfg(test)]
    pub(crate) fn new(time: DateTime<Utc>, value: f32) -> Self {
        Self { time, value }
    }
}

/// Merges pollen samples and AQI items into combined items.
///
/// The merging drops items from either the pollen samples or from the AQI items if they are not
/// stamped within half an hour of the first item of the latest starting series, thus lining them
/// before they are combined.
fn merge(
    pollen_samples: Vec<BuienradarSample>,
    aqi_items: Vec<LuchtmeetnetItem>,
) -> Result<Vec<Item>, MergeError> {
    let mut pollen_samples = pollen_samples;
    let mut aqi_items = aqi_items;

    // Only retain samples/items that have timestamps that are at least an hour ago.
    let now = Utc::now();
    pollen_samples.retain(|smp| smp.time.signed_duration_since(now).num_seconds() > -3600);
    aqi_items.retain(|item| item.time.signed_duration_since(now).num_seconds() > -3600);

    // Align the iterators based on the (hourly) timestamps!
    let pollen_first_time = pollen_samples
        .first()
        .ok_or(MergeError::NoPollenItemFound)?
        .time;
    let aqi_first_time = aqi_items.first().ok_or(MergeError::NoAqiItemFound)?.time;
    if pollen_first_time < aqi_first_time {
        // Drain one or more pollen samples to line up.
        let idx = pollen_samples
            .iter()
            .position(|smp| {
                smp.time
                    .signed_duration_since(aqi_first_time)
                    .num_seconds()
                    .abs()
                    < 1800
            })
            .ok_or(MergeError::NoCloseAqiItemFound)?;
        pollen_samples.drain(..idx);
    } else {
        // Drain one or more AQI items to line up.
        let idx = aqi_items
            .iter()
            .position(|item| {
                item.time
                    .signed_duration_since(pollen_first_time)
                    .num_seconds()
                    .abs()
                    < 1800
            })
            .ok_or(MergeError::NoClosePollenItemFound)?;
        aqi_items.drain(..idx);
    }

    // Combine the samples with items by taking the maximum of pollen sample score and AQI item
    // value.
    let items = pollen_samples
        .into_iter()
        .zip(aqi_items)
        .map(|(pollen_sample, aqi_item)| {
            let time = pollen_sample.time;
            let value = (pollen_sample.score as f32).max(aqi_item.value);

            Item { time, value }
        })
        .collect();

    Ok(items)
}

/// Retrieves the combined forecasted items for the provided position and metric.
///
/// It supports the following metric:
/// * [`Metric::PAQI`]
#[cached(
    time = 1800,
    key = "(Position, Metric)",
    convert = r#"{ (position, metric) }"#,
    result = true
)]
pub(crate) async fn get(
    position: Position,
    metric: Metric,
    maps_handle: &MapsHandle,
) -> Result<Vec<Item>, Error> {
    if metric != Metric::PAQI {
        return Err(Error::UnsupportedMetric(metric));
    };
    let pollen_items = buienradar::get_samples(position, Metric::Pollen, maps_handle).await?;
    let aqi_items = luchtmeetnet::get(position, Metric::AQI).await?;
    let items = merge(pollen_items, aqi_items)?;

    Ok(items)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Timelike};

    use super::*;

    #[test]
    fn merge() {
        let t_now = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let t_m2 = t_now.checked_sub_signed(Duration::days(1)).unwrap();
        let t_m1 = t_now.checked_sub_signed(Duration::hours(2)).unwrap();
        let t_0 = t_now.checked_add_signed(Duration::minutes(12)).unwrap();
        let t_1 = t_now.checked_add_signed(Duration::minutes(72)).unwrap();
        let t_2 = t_now.checked_add_signed(Duration::minutes(132)).unwrap();

        let pollen_samples = Vec::from([
            BuienradarSample::new(t_m2, 4),
            BuienradarSample::new(t_m1, 5),
            BuienradarSample::new(t_0, 1),
            BuienradarSample::new(t_1, 3),
            BuienradarSample::new(t_2, 2),
        ]);
        let aqi_items = Vec::from([
            LuchtmeetnetItem::new(t_m2, 4.0),
            LuchtmeetnetItem::new(t_m1, 5.0),
            LuchtmeetnetItem::new(t_0, 1.1),
            LuchtmeetnetItem::new(t_1, 2.9),
            LuchtmeetnetItem::new(t_2, 2.4),
        ]);

        // Perform a normal merge.
        let merged = super::merge(pollen_samples.clone(), aqi_items.clone());
        assert!(merged.is_ok());
        let paqi = merged.unwrap();
        assert_eq!(
            paqi,
            Vec::from([
                Item::new(t_0, 1.1),
                Item::new(t_1, 3.0),
                Item::new(t_2, 2.4),
            ])
        );

        // The pollen samples are shifted, i.e. one hour in the future.
        let shifted_pollen_samples = pollen_samples[2..]
            .iter()
            .cloned()
            .map(|mut item| {
                item.time = item.time.checked_add_signed(Duration::hours(1)).unwrap();
                item
            })
            .collect::<Vec<_>>();
        let merged = super::merge(shifted_pollen_samples, aqi_items.clone());
        assert!(merged.is_ok());
        let paqi = merged.unwrap();
        assert_eq!(paqi, Vec::from([Item::new(t_1, 2.9), Item::new(t_2, 3.0)]));

        // The AQI items are shifted, i.e. one hour in the future.
        let shifted_aqi_items = aqi_items[2..]
            .iter()
            .cloned()
            .map(|mut item| {
                item.time = item.time.checked_add_signed(Duration::hours(1)).unwrap();
                item
            })
            .collect::<Vec<_>>();
        let merged = super::merge(pollen_samples.clone(), shifted_aqi_items);
        assert!(merged.is_ok());
        let paqi = merged.unwrap();
        assert_eq!(paqi, Vec::from([Item::new(t_1, 3.0), Item::new(t_2, 2.9)]));

        // The maximum sample/item should not be later then the interval the PAQI items cover.
        let merged = super::merge(pollen_samples[..3].to_vec(), aqi_items.clone());
        assert!(merged.is_ok());
        let paqi = merged.unwrap();
        assert_eq!(paqi, Vec::from([Item::new(t_0, 1.1)]));

        let merged = super::merge(pollen_samples.clone(), aqi_items[..3].to_vec());
        assert!(merged.is_ok());
        let paqi = merged.unwrap();
        assert_eq!(paqi, Vec::from([Item::new(t_0, 1.1)]));

        // Merging fails because the samples/items are too far (6 hours) apart.
        let shifted_aqi_items = aqi_items
            .iter()
            .cloned()
            .map(|mut item| {
                item.time = item.time.checked_add_signed(Duration::hours(6)).unwrap();
                item
            })
            .collect::<Vec<_>>();
        let merged = super::merge(pollen_samples.clone(), shifted_aqi_items);
        assert_eq!(merged, Err(MergeError::NoCloseAqiItemFound));

        let shifted_pollen_samples = pollen_samples
            .iter()
            .cloned()
            .map(|mut item| {
                item.time = item.time.checked_add_signed(Duration::hours(6)).unwrap();
                item
            })
            .collect::<Vec<_>>();
        let merged = super::merge(shifted_pollen_samples, aqi_items.clone());
        assert_eq!(merged, Err(MergeError::NoClosePollenItemFound));

        // The pollen samples list is empty, or everything is too old.
        let merged = super::merge(Vec::new(), aqi_items.clone());
        assert_eq!(merged, Err(MergeError::NoPollenItemFound));
        let merged = super::merge(pollen_samples[0..2].to_vec(), aqi_items.clone());
        assert_eq!(merged, Err(MergeError::NoPollenItemFound));

        // The AQI items list is empty, or everything is too old.
        let merged = super::merge(pollen_samples.clone(), Vec::new());
        assert_eq!(merged, Err(MergeError::NoAqiItemFound));
        let merged = super::merge(pollen_samples, aqi_items[0..2].to_vec());
        assert_eq!(merged, Err(MergeError::NoAqiItemFound));
    }
}
