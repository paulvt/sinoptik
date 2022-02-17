//! Maps retrieval and caching.
//!
//! This module provides a task that keeps maps up-to-date using a maps-specific refresh interval.
//! It stores all the maps as [`DynamicImage`]s in memory.

// TODO: Allow dead code until either precipitation maps get used or dumped (#8).
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use chrono::DurationRound;
use image::{DynamicImage, ImageFormat};
use reqwest::Url;
use rocket::tokio;
use rocket::tokio::time::{sleep, Duration, Instant};

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

/// The base URL for retrieving the precipitation map from Weerplaza.
const PRECIPITATION_BASE_URL: &str =
    "https://cluster.api.meteoplaza.com/v3/nowcast/tiles/radarnl-forecast";

/// The interval for retrieving precipitation maps.
///
/// The series contains images for every 5 minutes, 24 in total.
const PRECIPITATION_INTERVAL: Duration = Duration::from_secs(300);

/// The number of precipitation maps retained.
const PRECIPITATION_MAP_COUNT: usize = 24;

/// The number of seconds each precipitation map is for.
const PRECIPITATION_MAP_INTERVAL: u64 = 300;

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

/// The `MapsRefresh` trait is used to reduce the time a lock needs to be held when updating maps.
///
/// When refreshing maps, the lock only needs to be held when checking whether a refresh is
/// necessary and when the new maps have been retrieved and can be updated.
trait MapsRefresh {
    /// Determines whether the pollen maps need to be refreshed.
    fn needs_pollen_refresh(&self) -> bool;

    /// Determines whether the precipitation maps need to be refreshed.
    fn needs_precipitation_refresh(&self) -> bool;

    /// Determines whether the UV index maps need to be refreshed.
    fn needs_uvi_refresh(&self) -> bool;

    /// Determines whether the pollen maps are stale.
    fn is_pollen_stale(&self) -> bool;

    /// Determines whether the precipitation maps are stale.
    fn is_precipitation_stale(&self) -> bool;

    /// Determines whether the UV index maps are stale.
    fn is_uvi_stale(&self) -> bool;

    /// Updates the pollen maps.
    fn set_pollen(&self, pollen: Option<DynamicImage>);

    /// Updates the precipitation maps.
    fn set_precipitation(&self, precipitation: [Option<DynamicImage>; PRECIPITATION_MAP_COUNT]);

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

    /// The precipitation maps (from Weerplaza).
    // TODO: Make one large image instead of using an array? This is already the case for the
    //   other maps.
    pub(crate) precipitation: [Option<DynamicImage>; PRECIPITATION_MAP_COUNT],

    /// The timestamp the precipitation maps were last refreshed.
    precipitation_stamp: Instant,

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
        // Because `Option<DynamicImage>` does not implement `Copy`
        let precipitation = [(); PRECIPITATION_MAP_COUNT].map(|_| None);

        Self {
            pollen: None,
            pollen_stamp: now,
            precipitation,
            precipitation_stamp: now,
            uvi: None,
            uvi_stamp: now,
        }
    }

    /// Returns the pollen map for the given instant.
    ///
    /// This returns [`None`] if the map is not in the cache yet, or if `instant` is too far in the
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

    /// Returns the precipitation map for the given instant.
    ///
    /// This returns [`None`] if the map is not in the cache yet, or if `instant` is too far in the
    /// future with respect to the cached maps.
    pub(crate) fn precipitation_at(&self, instant: Instant) -> Option<DynamicImage> {
        let duration = instant.duration_since(self.precipitation_stamp);
        let offset = (duration.as_secs() / PRECIPITATION_MAP_INTERVAL) as usize;
        // Check if out of bounds.
        if offset >= PRECIPITATION_MAP_COUNT {
            return None;
        }

        self.precipitation[offset].as_ref().map(Clone::clone)
    }

    /// Returns the UV index map for the given instant.
    ///
    /// This returns [`None`] if the map is not in the cache yet, or if `instant` is too far in
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
}

impl MapsRefresh for MapsHandle {
    fn is_pollen_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        Instant::now().duration_since(maps.pollen_stamp)
            > Duration::from_secs(POLLEN_MAP_COUNT as u64 * POLLEN_MAP_INTERVAL)
    }

    fn is_precipitation_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        Instant::now().duration_since(maps.precipitation_stamp)
            > Duration::from_secs(PRECIPITATION_MAP_COUNT as u64 * PRECIPITATION_MAP_INTERVAL)
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

    fn needs_precipitation_refresh(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");
        maps.precipitation.iter().any(|map| map.is_none())
            || Instant::now().duration_since(maps.precipitation_stamp) > PRECIPITATION_INTERVAL
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

    fn set_precipitation(&self, precipitation: [Option<DynamicImage>; 24]) {
        // If the first map is present, it is already worth setting it.
        if precipitation[0].is_some() || self.is_precipitation_stale() {
            let mut maps = self.lock().expect("Maps handle mutex was poisoned");
            maps.precipitation = precipitation;
            maps.precipitation_stamp = Instant::now();
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

/// Retrieves the pollen maps from Weerplaza.
///
/// See [`PRECIPITATION_BASE_URL`] for the base URL and [`retrieve_image`] for the retrieval
/// function.
async fn retrieve_precipitation_maps() -> [Option<DynamicImage>; 24] {
    let just_before = (chrono::Utc::now() - chrono::Duration::minutes(10))
        // This only fails if timestamps and durations exceed limits!
        .duration_trunc(chrono::Duration::minutes(5))
        .unwrap();
    let timestamp_prefix = just_before.format("%Y%m%d%H%M");
    let base_url = Url::parse(PRECIPITATION_BASE_URL).unwrap();

    let mut precipitation: [Option<DynamicImage>; 24] = Default::default();
    for (index, map) in precipitation.iter_mut().enumerate() {
        let timestamp = format!("{timestamp_prefix}_{:03}", index * 5);
        let mut url = base_url.clone();
        url.path_segments_mut().unwrap().push(&timestamp);

        println!("🔽 Refreshing precipitation map from: {}", url);
        *map = retrieve_image(url).await;
    }

    precipitation
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
        // Disable for now, they are not used.
        // if maps_handle.needs_precipitation_refresh() {
        //     let precipitation = retrieve_precipitation_maps().await;
        //     maps_handle.set_precipitation(precipitation);
        // }
        if maps_handle.needs_uvi_refresh() {
            let uvi = retrieve_uvi_maps().await;
            maps_handle.set_uvi(uvi);
        }

        sleep(REFRESH_INTERVAL).await;
    }
}
