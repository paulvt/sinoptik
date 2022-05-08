//! The combined data provider.
//!
//! This combines and collates data using the other providers.

use cached::proc_macro::cached;
use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use rocket::serde::Serialize;

pub(crate) use super::buienradar::{self, Sample as BuienradarSample};
pub(crate) use super::luchtmeetnet::{self, Item as LuchtmeetnetItem};
use crate::maps::MapsHandle;
use crate::position::Position;
use crate::Metric;

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

/// Merges pollen samples and AQI items into combined items.
///
/// The merging drops items from either the pollen samples or from the AQI items if they are not
/// stamped with half an hour of the first item of the latest starting series, thus lining them
/// before they are combined.
///
/// This function also finds the maximum pollen sample and AQI item.
///
/// Returns [`None`] if there are no pollen samples, if there are no AQI items, or if
/// lining them up fails. Returns [`None`] for the maximum pollen sample or maximum AQI item
/// if there are no samples or items.
fn merge(
    pollen_samples: Vec<BuienradarSample>,
    aqi_items: Vec<LuchtmeetnetItem>,
) -> Option<(
    Vec<Item>,
    Option<BuienradarSample>,
    Option<LuchtmeetnetItem>,
)> {
    let mut pollen_samples = pollen_samples;
    let mut aqi_items = aqi_items;

    // Only retain samples/items that have timestamps that are at least half an hour ago.
    let now = Utc::now();
    pollen_samples.retain(|smp| smp.time.signed_duration_since(now).num_seconds() > -1800);
    aqi_items.retain(|item| item.time.signed_duration_since(now).num_seconds() > -1800);

    // Align the iterators based on the (hourly) timestamps!
    let pollen_first_time = pollen_samples.first()?.time;
    let aqi_first_time = aqi_items.first()?.time;
    if pollen_first_time < aqi_first_time {
        // Drain one or more pollen samples to line up.
        let idx = pollen_samples.iter().position(|smp| {
            smp.time
                .signed_duration_since(aqi_first_time)
                .num_seconds()
                .abs()
                < 1800
        })?;
        pollen_samples.drain(..idx);
    } else {
        // Drain one or more AQI items to line up.
        let idx = aqi_items.iter().position(|item| {
            item.time
                .signed_duration_since(pollen_first_time)
                .num_seconds()
                .abs()
                < 1800
        })?;
        aqi_items.drain(..idx);
    }

    // Find the maximum sample/item of each series.
    let pollen_max = pollen_samples
        .iter()
        .max_by_key(|sample| sample.score)
        .cloned();
    let aqi_max = aqi_items
        .iter()
        .max_by_key(|item| (item.value * 1_000.0) as u32)
        .cloned();

    // Combine the samples with items by taking the maximum of pollen sample score and AQI item
    // value.
    let items = pollen_samples
        .into_iter()
        .zip(aqi_items.into_iter())
        .map(|(pollen_sample, aqi_item)| {
            let time = pollen_sample.time;
            let value = (pollen_sample.score as f32).max(aqi_item.value);

            Item { time, value }
        })
        .collect();

    Some((items, pollen_max, aqi_max))
}

/// Retrieves the combined forecasted items for the provided position and metric.
///
/// Besides the combined items, it also yields the maxium pollen sample and AQI item.
/// Note that the maximum values are calculated before combining them, so the time stamp
/// corresponds to the one in the original series, not to a timestamp of an item after merging.
///
/// It supports the following metric:
/// * [`Metric::PAQI`]
///
/// Returns [`None`] for the combined items if retrieving data from either the Buienradar or the
/// Luchtmeetnet provider fails or if they cannot be combined. Returns [`None`] for the maxiumum
/// pollen sample or AQI item if there are no samples or items.
///
/// If the result is [`Some`], it will be cached for 30 minutes for the the given position and
/// metric.
#[cached(
    time = 1800,
    key = "(Position, Metric)",
    convert = r#"{ (position, metric) }"#,
    option = true
)]
pub(crate) async fn get(
    position: Position,
    metric: Metric,
    maps_handle: &MapsHandle,
) -> Option<(
    Vec<Item>,
    Option<BuienradarSample>,
    Option<LuchtmeetnetItem>,
)> {
    if metric != Metric::PAQI {
        return None;
    };
    let pollen_items = buienradar::get_samples(position, Metric::Pollen, maps_handle).await;
    let aqi_items = luchtmeetnet::get(position, Metric::AQI).await;

    merge(pollen_items?, aqi_items?)
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
            BuienradarSample {
                time: t_m2,
                score: 4,
            },
            BuienradarSample {
                time: t_m1,
                score: 5,
            },
            BuienradarSample {
                time: t_0,
                score: 1,
            },
            BuienradarSample {
                time: t_1,
                score: 3,
            },
            BuienradarSample {
                time: t_2,
                score: 2,
            },
        ]);
        let aqi_items = Vec::from([
            LuchtmeetnetItem {
                time: t_m2,
                value: 4.0,
            },
            LuchtmeetnetItem {
                time: t_m1,
                value: 5.0,
            },
            LuchtmeetnetItem {
                time: t_0,
                value: 1.1,
            },
            LuchtmeetnetItem {
                time: t_1,
                value: 2.9,
            },
            LuchtmeetnetItem {
                time: t_2,
                value: 2.4,
            },
        ]);

        // A normal merge.
        let merged = super::merge(pollen_samples.clone(), aqi_items.clone());
        assert!(merged.is_some());
        let (paqi, max_pollen, max_aqi) = merged.unwrap();
        assert_eq!(
            paqi,
            Vec::from([
                Item {
                    time: t_0,
                    value: 1.1
                },
                Item {
                    time: t_1,
                    value: 3.0
                },
                Item {
                    time: t_2,
                    value: 2.4
                },
            ])
        );
        assert_eq!(
            max_pollen,
            Some(BuienradarSample {
                time: t_1,
                score: 3
            })
        );
        assert_eq!(
            max_aqi,
            Some(LuchtmeetnetItem {
                time: t_1,
                value: 2.9
            })
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
        assert!(merged.is_some());
        let (paqi, max_pollen, max_aqi) = merged.unwrap();
        assert_eq!(
            paqi,
            Vec::from([
                Item {
                    time: t_1,
                    value: 2.9
                },
                Item {
                    time: t_2,
                    value: 3.0
                }
            ])
        );
        assert_eq!(
            max_pollen,
            Some(BuienradarSample {
                time: t_2,
                score: 3
            })
        );
        assert_eq!(
            max_aqi,
            Some(LuchtmeetnetItem {
                time: t_1,
                value: 2.9
            })
        );

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
        assert!(merged.is_some());
        let (paqi, max_pollen, max_aqi) = merged.unwrap();
        assert_eq!(
            paqi,
            Vec::from([
                Item {
                    time: t_1,
                    value: 3.0
                },
                Item {
                    time: t_2,
                    value: 2.9
                }
            ])
        );
        assert_eq!(
            max_pollen,
            Some(BuienradarSample {
                time: t_1,
                score: 3
            })
        );
        assert_eq!(
            max_aqi,
            Some(LuchtmeetnetItem {
                time: t_2,
                value: 2.9
            })
        );

        // Merging fails because the samples/items are too far apart.
        let shifted_aqi_items = aqi_items
            .iter()
            .cloned()
            .map(|mut item| {
                item.time = item.time.checked_add_signed(Duration::hours(6)).unwrap();
                item
            })
            .collect::<Vec<_>>();
        let merged = super::merge(pollen_samples.clone(), shifted_aqi_items);
        assert_eq!(merged, None);

        // The pollen samples list is empty, or everything is too old.
        let merged = super::merge(Vec::new(), aqi_items.clone());
        assert_eq!(merged, None);
        let merged = super::merge(pollen_samples[0..2].to_vec(), aqi_items.clone());
        assert_eq!(merged, None);

        // The AQI items list is empty, or everything is too old.
        let merged = super::merge(pollen_samples.clone(), Vec::new());
        assert_eq!(merged, None);
        let merged = super::merge(pollen_samples, aqi_items[0..2].to_vec());
        assert_eq!(merged, None);
    }
}
