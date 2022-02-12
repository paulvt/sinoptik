//! Maps retrieval and caching.
//!
//! This module provides a task that keeps maps up-to-date using a maps-specific refresh interval.
//! It stores all the maps as [`DynamicImage`]s in memory.

use chrono::DurationRound;
use image::DynamicImage;
use rocket::tokio::time::{sleep, Duration, Instant};

use crate::MapsHandle;

/// The interval between map refreshes (in seconds).
const SLEEP_INTERVAL: Duration = Duration::from_secs(60);

/// The base URL for retrieving the precipitation map.
const PRECIPITATION_BASE_URL: &str =
    "https://cluster.api.meteoplaza.com/v3/nowcast/tiles/radarnl-forecast";

/// The interval for retrieving precipitation maps.
const PRECIPITATION_INTERVAL: Duration = Duration::from_secs(300);

/// The base URL for retrieving the pollen maps.
const POLLEN_BASE_URL: &str =
    "https://image.buienradar.nl/2.0/image/sprite/WeatherMapPollenRadarHourlyNL\
        ?height=988&width=820&extension=png&renderBackground=False&renderBranding=False\
        &renderText=False&history=0&forecast=24&skip=0&timestamp=";

/// The interval for retrieving pollen maps.
const POLLEN_INTERVAL: Duration = Duration::from_secs(600);

/// The base URL for retrieving the UV index maps.
const UVI_BASE_URL: &str = "https://image.buienradar.nl/2.0/image/sprite/WeatherMapUVIndexNL\
        ?extension=png&width=820&height=988&renderText=False&renderBranding=False\
        &renderBackground=False&history=0&forecast=5&skip=0&timestamp=";

/// The interval for retrieving UV index maps.
const UVI_INTERVAL: Duration = Duration::from_secs(3600);

/// Runs a loop that keeps refreshing the maps when necessary.
pub(crate) async fn run(maps_handle: MapsHandle) -> ! {
    loop {
        println!("🕔 Refreshing the maps (if necessary)...");

        // FIXME: Refactor this so that the lock is only held when updating the maps fields.
        maps_handle.lock().await.refresh_precipitation().await;
        maps_handle.lock().await.refresh_pollen().await;
        maps_handle.lock().await.refresh_uvi().await;

        sleep(SLEEP_INTERVAL).await;
    }
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
    pub(crate) precipitation: [Option<DynamicImage>; 24],

    /// The timestamp the precipitation maps were last refreshed.
    precipitation_stamp: Instant,

    /// The UV index maps (from Buienradar).
    pub(crate) uvi: Option<DynamicImage>,

    /// The timestamp the UV index maps were last refreshed.
    uvi_stamp: Instant,
}

impl Maps {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        // Because `Option<DynamicImage>` does not implement `Copy`
        let precipitation = [(); 24].map(|_| None);

        Self {
            pollen: None,
            pollen_stamp: now,
            precipitation,
            precipitation_stamp: now,
            uvi: None,
            uvi_stamp: now,
        }
    }

    async fn refresh_precipitation(&mut self) {
        if self.precipitation.iter().any(|map| map.is_none())
            || Instant::now().duration_since(self.precipitation_stamp) > PRECIPITATION_INTERVAL
        {
            let just_before = (chrono::Utc::now() - chrono::Duration::minutes(10))
                // This only fails if timestamps and durations exceed limits!
                .duration_trunc(chrono::Duration::minutes(5))
                .unwrap();
            let timestamp = just_before.format("%Y%m%d%H%M");

            for k in 0..24 {
                let suffix = format!("{:03}", k * 5);
                let url = format!("{PRECIPITATION_BASE_URL}/{timestamp}_{suffix}");
                println!("🔽 Refreshing precipitation maps from: {}", url);
                self.precipitation[k] = retrieve_image(&url).await;
            }
            self.precipitation_stamp = Instant::now();
        }
    }

    async fn refresh_pollen(&mut self) {
        if self.pollen.is_none()
            || Instant::now().duration_since(self.pollen_stamp) > POLLEN_INTERVAL
        {
            let timestamp = chrono::Local::now().format("%y%m%d%H%M");
            let url = format!("{POLLEN_BASE_URL}{timestamp}");

            println!("🔽 Refreshing pollen maps from: {}", url);
            self.pollen = retrieve_image(&url).await;
            self.pollen_stamp = Instant::now();
        }
    }

    async fn refresh_uvi(&mut self) {
        if self.uvi.is_none() || Instant::now().duration_since(self.uvi_stamp) > UVI_INTERVAL {
            let timestamp = chrono::Local::now().format("%y%m%d%H%M");
            let url = format!("{UVI_BASE_URL}{timestamp}");

            println!("🔽 Refreshing UV index maps from: {}", url);
            self.uvi = retrieve_image(&url).await;
            self.uvi_stamp = Instant::now();
        }
    }
}

/// Retrieves an image from the provided URL.
async fn retrieve_image(url: &str) -> Option<DynamicImage> {
    // TODO: Handle or log errors!
    let response = reqwest::get(url).await.ok()?;
    let bytes = response.bytes().await.ok()?;

    image::load_from_memory(&bytes).ok()
}
