//! Maps retrieval and caching.
//!
//! This module provides a task that keeps maps up-to-date using a maps-specific refresh interval.
//! It stores all the maps as [`DynamicImage`]s in memory.

// TODO: Allow dead code until #9 is implemented.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use image::{DynamicImage, ImageFormat};
use reqwest::Url;
use rocket::serde::Serialize;
use rocket::tokio;
use rocket::tokio::time::{sleep, Duration, Instant};

use crate::position::Position;

/// A handle to access the in-memory cached maps.
pub(crate) type MapsHandle = Arc<Mutex<Maps>>;

/// The interval between map refreshes (in seconds).
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// The base URL for retrieving the pollen maps from Buienradar.
const POLLEN_BASE_URL: &str =
    "https://image.buienradar.nl/2.0/image/sprite/WeatherMapPollenRadarHourlyNL\
        ?width=820&height=988&extension=png&renderBackground=False&renderBranding=False\
        &renderText=False&history=0&forecast=24&skip=0";

/// The interval for retrieving pollen maps.
///
/// The endpoint provides a map for every hour, 24 in total.
const POLLEN_INTERVAL: Duration = Duration::from_secs(3_600);

/// The number of pollen maps retained.
const POLLEN_MAP_COUNT: u32 = 24;

/// The number of seconds each pollen map is for.
const POLLEN_MAP_INTERVAL: u64 = 3_600;

/// The position reference points for the pollen map.
///
/// Maps the gecoded positions of two reference points as follows:
/// * Latitude and longitude of Vlissingen to its y- and x-position
/// * Latitude of Lauwersoog to its y-position and longitude of Enschede to its x-position
const POLLEN_MAP_REF_POINTS: [(Position, (u32, u32)); 2] = [
    (Position::new(5.1, 3.57), (84, 745)),  // Vlissingen
    (Position::new(53.4, 6.9), (111, 694)), // Lauwersoog (lat/y) and Enschede (lon/x)
];

/// The base URL for retrieving the UV index maps from Buienradar.
const UVI_BASE_URL: &str = "https://image.buienradar.nl/2.0/image/sprite/WeatherMapUVIndexNL\
        ?width=820&height=988&extension=png&&renderBackground=False&renderBranding=False\
        &renderText=False&history=0&forecast=5&skip=0";

/// The interval for retrieving UV index maps.
///
/// The endpoint provides a map for every day, 5 in total.
const UVI_INTERVAL: Duration = Duration::from_secs(24 * 3_600);

/// The number of UV index maps retained.
const UVI_MAP_COUNT: u32 = 5;

/// The number of seconds each UV index map is for.
const UVI_MAP_INTERVAL: u64 = 24 * 3_600;

/// The position reference points for the UV index map.
const POLLEN_UVI_REF_POINT: [(Position, (u32, u32)); 2] = POLLEN_MAP_REF_POINTS;

/// The `MapsRefresh` trait is used to reduce the time a lock needs to be held when updating maps.
///
/// When refreshing maps, the lock only needs to be held when checking whether a refresh is
/// necessary and when the new maps have been retrieved and can be updated.
trait MapsRefresh {
    /// Determines whether the pollen maps need to be refreshed.
    fn needs_pollen_refresh(&self) -> bool;

    /// Determines whether the UV index maps need to be refreshed.
    fn needs_uvi_refresh(&self) -> bool;

    /// Determines whether the pollen maps are stale.
    fn is_pollen_stale(&self) -> bool;

    /// Determines whether the UV index maps are stale.
    fn is_uvi_stale(&self) -> bool;

    /// Updates the pollen maps.
    fn set_pollen(&self, pollen: Option<DynamicImage>);

    /// Updates the UV index maps.
    fn set_uvi(&self, uvi: Option<DynamicImage>);
}

/// Container type for all in-memory cached maps.
#[derive(Debug)]
pub(crate) struct Maps {
    /// The pollen maps (from Buienradar).
    pub(crate) pollen: Option<DynamicImage>,

    /// The timestamp the pollen maps were last refreshed.
    pollen_stamp: Instant,

    /// The UV index maps (from Buienradar).
    pub(crate) uvi: Option<DynamicImage>,

    /// The timestamp the UV index maps were last refreshed.
    uvi_stamp: Instant,
}

impl Maps {
    /// Creates a new maps cache.
    ///
    /// It contains an [`DynamicImage`] per maps type, if downloaded, and the timestamp of the last
    /// update.
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            pollen: None,
            pollen_stamp: now,
            uvi: None,
            uvi_stamp: now,
        }
    }

    /// Returns the pollen map for the given instant.
    ///
    /// This returns [`None`] if the maps are not in the cache yet, or if `instant` is too far in the
    /// future with respect to the cached maps.
    pub(crate) fn pollen_at(&self, instant: Instant) -> Option<DynamicImage> {
        let duration = instant.duration_since(self.pollen_stamp);
        let offset = (duration.as_secs() / POLLEN_MAP_INTERVAL) as u32;
        // Check if out of bounds.
        if offset >= POLLEN_MAP_COUNT {
            return None;
        }

        self.pollen.as_ref().map(|map| {
            let width = map.width() / POLLEN_MAP_COUNT;

            map.crop_imm(offset * width, 0, width, map.height())
        })
    }

    /// Projects the provided geocoded position to a coordinat on a pollen map.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    fn pollen_project(&self, _position: Position) -> Option<(u32, u32)> {
        // TODO: Use map width (with `POLLEN_MAP_COUNT`), map height and `POLLEN_MAP_REF_POINTS`.
        //   Then, call shared function with `uvi_sample`.
        todo!();
    }

    /// Samples the pollen maps for the given position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    /// Otherwise, it returns [`Some`] with a list of pollen sample, one for each map
    /// in the series of maps.
    pub(crate) fn pollen_sample(&self, _position: Position) -> Option<Vec<PollenSample>> {
        // TODO: Sample each map using the projected coordinates from the pollen map
        //   timestamp, yielding it for each `POLLEN_MAP_INTERVAL`.
        todo!()
    }

    /// Returns the UV index map for the given instant.
    ///
    /// This returns [`None`] if the maps are not in the cache yet, or if `instant` is too far in
    /// the future with respect to the cached maps.
    pub(crate) fn uvi_at(&self, instant: Instant) -> Option<DynamicImage> {
        let duration = instant.duration_since(self.uvi_stamp);
        let offset = (duration.as_secs() / UVI_MAP_INTERVAL) as u32;
        // Check if out of bounds.
        if offset >= UVI_MAP_COUNT {
            return None;
        }

        self.uvi.as_ref().map(|map| {
            let width = map.width() / UVI_MAP_COUNT;

            map.crop_imm(offset * width, 0, width, map.height())
        })
    }

    /// Projects the provided geocoded position to a coordinat on an UV index map.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    fn uvi_project(&self, _position: Position) -> Option<(u32, u32)> {
        // TODO: Use map width (with `UVI_MAP_COUNT`), map height and `UVI_MAP_REF_POINTS`.
        //   Then, call shared function with `pollen_sample`.
        todo!();
    }

    /// Samples the UV index maps for the given position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    /// Otherwise, it returns [`Some`] with a list of UV index sample, one for each map
    /// in the series of maps.
    pub(crate) fn uvi_sample(&self, _position: Position) -> Option<Vec<UviSample>> {
        // TODO: Sample each map using the projected coordinates from the UV index map
        //   timestamp, yielding it for each `UVI_MAP_INTERVAL`.
        todo!()
    }
}

impl MapsRefresh for MapsHandle {
    fn is_pollen_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        Instant::now().duration_since(maps.pollen_stamp)
            > Duration::from_secs(POLLEN_MAP_COUNT as u64 * POLLEN_MAP_INTERVAL)
    }

    fn is_uvi_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        Instant::now().duration_since(maps.uvi_stamp)
            > Duration::from_secs(UVI_MAP_COUNT as u64 * UVI_MAP_INTERVAL)
    }

    fn needs_pollen_refresh(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");
        maps.pollen.is_none() || Instant::now().duration_since(maps.pollen_stamp) > POLLEN_INTERVAL
    }

    fn needs_uvi_refresh(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");
        maps.uvi.is_none() || Instant::now().duration_since(maps.uvi_stamp) > UVI_INTERVAL
    }

    fn set_pollen(&self, pollen: Option<DynamicImage>) {
        if pollen.is_some() || self.is_pollen_stale() {
            let mut maps = self.lock().expect("Maps handle mutex was poisoned");
            maps.pollen = pollen;
            maps.pollen_stamp = Instant::now();
        }
    }

    fn set_uvi(&self, uvi: Option<DynamicImage>) {
        if uvi.is_some() || self.is_uvi_stale() {
            let mut maps = self.lock().expect("Maps handle mutex was poisoned");
            maps.uvi = uvi;
            maps.uvi_stamp = Instant::now();
        }
    }
}

/// A Buienradar pollen map sample.
///
/// This represents a value at a given time.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct PollenSample {
    /// The time(stamp) of the forecast.
    #[serde(serialize_with = "ts_seconds::serialize")]
    time: DateTime<Utc>,

    /// The forecasted value.
    ///
    /// A value in the range `1..=10`.
    value: u8,
}

/// A Buienradar UV index map sample.
///
/// This represents a value at a given time.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct UviSample {
    /// The time(stamp) of the forecast.
    #[serde(serialize_with = "ts_seconds::serialize")]
    time: DateTime<Utc>,

    /// The forecasted value.
    ///
    /// A value in the range `1..=10`.
    value: u8,
}

/// Retrieves an image from the provided URL.
///
/// This returns [`None`] if it fails in either performing the request, retrieving the bytes from
/// the image or loading and the decoding the data into [`DynamicImage`].
async fn retrieve_image(url: Url) -> Option<DynamicImage> {
    // TODO: Handle or log errors!
    let response = reqwest::get(url).await.ok()?;
    let bytes = response.bytes().await.ok()?;

    tokio::task::spawn_blocking(move || {
        image::load_from_memory_with_format(&bytes, ImageFormat::Png)
    })
    .await
    .ok()?
    .ok()
}

/// Retrieves the pollen maps from Buienradar.
///
/// See [`POLLEN_BASE_URL`] for the base URL and [`retrieve_image`] for the retrieval function.
async fn retrieve_pollen_maps() -> Option<DynamicImage> {
    let timestamp = format!("{}", chrono::Local::now().format("%y%m%d%H%M"));
    let mut url = Url::parse(POLLEN_BASE_URL).unwrap();
    url.query_pairs_mut().append_pair("timestamp", &timestamp);

    println!("🔽 Refreshing pollen maps from: {}", url);
    retrieve_image(url).await
}

/// Retrieves the UV index maps from Buienradar.
///
/// See [`UVI_BASE_URL`] for the base URL and [`retrieve_image`] for the retrieval function.
async fn retrieve_uvi_maps() -> Option<DynamicImage> {
    let timestamp = format!("{}", chrono::Local::now().format("%y%m%d%H%M"));
    let mut url = Url::parse(UVI_BASE_URL).unwrap();
    url.query_pairs_mut().append_pair("timestamp", &timestamp);

    println!("🔽 Refreshing UV index maps from: {}", url);
    retrieve_image(url).await
}

/// Runs a loop that keeps refreshing the maps when necessary.
///
/// Use [`MapsRefresh`] trait methods on `maps_handle` to check whether each maps type needs to be
/// refreshed and uses its retrieval function to update it if necessary.
pub(crate) async fn run(maps_handle: MapsHandle) -> ! {
    loop {
        println!("🕔 Refreshing the maps (if necessary)...");

        if maps_handle.needs_pollen_refresh() {
            let pollen = retrieve_pollen_maps().await;
            maps_handle.set_pollen(pollen);
        }

        if maps_handle.needs_uvi_refresh() {
            let uvi = retrieve_uvi_maps().await;
            maps_handle.set_uvi(uvi);
        }

        sleep(REFRESH_INTERVAL).await;
    }
}
