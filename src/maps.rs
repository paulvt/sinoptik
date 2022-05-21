//! Maps retrieval and caching.
//!
//! This module provides a task that keeps maps up-to-date using a maps-specific refresh interval.
//! It stores all the maps as [`DynamicImage`]s in memory.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};

use chrono::serde::ts_seconds;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use image::{DynamicImage, GenericImage, GenericImageView, ImageFormat, Pixel, Rgb, Rgba};
use reqwest::Url;
use rocket::serde::Serialize;
use rocket::tokio;
use rocket::tokio::time::sleep;

use crate::forecast::Metric;
use crate::position::Position;

/// A handle to access the in-memory cached maps.
pub(crate) type MapsHandle = Arc<Mutex<Maps>>;

/// A histogram mapping map key colors to occurences/counts.
type MapKeyHistogram = HashMap<Rgb<u8>, u32>;

/// The Buienradar map key used for determining the score of a coordinate by mapping its color.
///
/// Note that the actual score starts from 1, not 0 as per this array.
#[rustfmt::skip]
const MAP_KEY: [[u8; 3]; 10] = [
    [0x49, 0xDA, 0x21],
    [0x30, 0xD2, 0x00],
    [0xFF, 0xF8, 0x8B],
    [0xFF, 0xF6, 0x42],
    [0xFD, 0xBB, 0x31],
    [0xFD, 0x8E, 0x24],
    [0xFC, 0x10, 0x3E],
    [0x97, 0x0A, 0x33],
    [0xA6, 0x6D, 0xBC],
    [0xB3, 0x30, 0xA1],
];

/// The Buienradar map sample size.
///
/// Determiess the number of pixels in width/height that is samples around the sampling coordinate.
const MAP_SAMPLE_SIZE: [u32; 2] = [11, 11];

/// The interval between map refreshes (in seconds).
const REFRESH_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(60);

/// The base URL for retrieving the pollen maps from Buienradar.
const POLLEN_BASE_URL: &str =
    "https://image.buienradar.nl/2.0/image/sprite/WeatherMapPollenRadarHourlyNL\
        ?width=820&height=988&extension=png&renderBackground=False&renderBranding=False\
        &renderText=False&history=0&forecast=24&skip=0";

/// The interval for retrieving pollen maps.
///
/// The endpoint provides a map for every hour, 24 in total.
const POLLEN_INTERVAL: i64 = 3_600;

/// The number of pollen maps retained.
const POLLEN_MAP_COUNT: u32 = 24;

/// The number of seconds each pollen map is for.
const POLLEN_MAP_INTERVAL: i64 = 3_600;

/// The position reference points for the pollen map.
///
/// Maps the gecoded positions of two reference points as follows:
/// * Latitude and longitude of Vlissingen to its y- and x-position
/// * Latitude of Lauwersoog to its y-position and longitude of Enschede to its x-position
const POLLEN_MAP_REF_POINTS: [(Position, (u32, u32)); 2] = [
    (Position::new(51.44, 3.57), (745, 84)),  // Vlissingen
    (Position::new(53.40, 6.90), (111, 694)), // Lauwersoog (lat/y) and Enschede (lon/x)
];

/// The base URL for retrieving the UV index maps from Buienradar.
const UVI_BASE_URL: &str = "https://image.buienradar.nl/2.0/image/sprite/WeatherMapUVIndexNL\
        ?width=820&height=988&extension=png&&renderBackground=False&renderBranding=False\
        &renderText=False&history=0&forecast=5&skip=0";

/// The interval for retrieving UV index maps.
///
/// The endpoint provides a map for every day, 5 in total.
const UVI_INTERVAL: i64 = 24 * 3_600;

/// The number of UV index maps retained.
const UVI_MAP_COUNT: u32 = 5;

/// The number of seconds each UV index map is for.
const UVI_MAP_INTERVAL: i64 = 24 * 3_600;

/// The position reference points for the UV index map.
const UVI_MAP_REF_POINTS: [(Position, (u32, u32)); 2] = POLLEN_MAP_REF_POINTS;

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
    fn set_pollen(&self, result: Option<RetrievedMaps>);

    /// Updates the UV index maps.
    fn set_uvi(&self, result: Option<RetrievedMaps>);
}

/// Container type for all in-memory cached maps.
#[derive(Debug, Default)]
pub(crate) struct Maps {
    /// The pollen maps (from Buienradar).
    pub(crate) pollen: Option<RetrievedMaps>,

    /// The UV index maps (from Buienradar).
    pub(crate) uvi: Option<RetrievedMaps>,
}

impl Maps {
    /// Creates a new maps cache.
    ///
    /// It contains an [`DynamicImage`] per maps type, if downloaded, and the timestamp of the last
    /// update.
    pub(crate) fn new() -> Self {
        Self {
            pollen: None,
            uvi: None,
        }
    }

    /// Returns a current pollen map that marks the provided position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet, there is no matching map for
    /// the current moment or if the provided position is not within the bounds of the map.
    pub(crate) fn pollen_mark(&self, position: Position) -> Option<DynamicImage> {
        self.pollen.as_ref().and_then(|maps| {
            let image = &maps.image;
            let stamp = maps.timestamp_base;
            let marked_image = map_at(
                image,
                stamp,
                POLLEN_MAP_INTERVAL,
                POLLEN_MAP_COUNT,
                Utc::now(),
            )?;
            let coords = project(&marked_image, POLLEN_MAP_REF_POINTS, position)?;

            Some(mark(marked_image, coords))
        })
    }

    /// Samples the pollen maps for the given position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    /// Otherwise, it returns [`Some`] with a list of pollen sample, one for each map
    /// in the series of maps.
    pub(crate) fn pollen_samples(&self, position: Position) -> Option<Vec<Sample>> {
        self.pollen.as_ref().and_then(|maps| {
            let image = &maps.image;
            let map = image.view(0, 0, image.width() / UVI_MAP_COUNT, image.height());
            let coords = project(&*map, POLLEN_MAP_REF_POINTS, position)?;
            let stamp = maps.timestamp_base;

            sample(image, stamp, POLLEN_MAP_INTERVAL, POLLEN_MAP_COUNT, coords)
        })
    }

    /// Returns a current UV index map that marks the provided position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet, there is no matching map for
    /// the current moment or if the provided position is not within the bounds of the map.
    pub(crate) fn uvi_mark(&self, position: Position) -> Option<DynamicImage> {
        self.uvi.as_ref().and_then(|maps| {
            let image = &maps.image;
            let stamp = maps.timestamp_base;
            let marked_image = map_at(image, stamp, UVI_MAP_INTERVAL, UVI_MAP_COUNT, Utc::now())?;
            let coords = project(&marked_image, POLLEN_MAP_REF_POINTS, position)?;

            Some(mark(marked_image, coords))
        })
    }

    /// Samples the UV index maps for the given position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    /// Otherwise, it returns [`Some`] with a list of UV index sample, one for each map
    /// in the series of maps.
    pub(crate) fn uvi_samples(&self, position: Position) -> Option<Vec<Sample>> {
        self.uvi.as_ref().and_then(|maps| {
            let image = &maps.image;
            let map = image.view(0, 0, image.width() / UVI_MAP_COUNT, image.height());
            let coords = project(&*map, UVI_MAP_REF_POINTS, position)?;
            let stamp = maps.timestamp_base;

            sample(image, stamp, UVI_MAP_INTERVAL, UVI_MAP_COUNT, coords)
        })
    }
}

impl MapsRefresh for MapsHandle {
    fn is_pollen_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        match &maps.pollen {
            Some(pollen_maps) => {
                Utc::now().signed_duration_since(pollen_maps.mtime)
                    > Duration::seconds(POLLEN_MAP_COUNT as i64 * POLLEN_MAP_INTERVAL)
            }
            None => false,
        }
    }

    fn is_uvi_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        match &maps.uvi {
            Some(uvi_maps) => {
                Utc::now().signed_duration_since(uvi_maps.mtime)
                    > Duration::seconds(UVI_MAP_COUNT as i64 * UVI_MAP_INTERVAL)
            }
            None => false,
        }
    }

    fn needs_pollen_refresh(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        match &maps.pollen {
            Some(pollen_maps) => {
                Utc::now()
                    .signed_duration_since(pollen_maps.mtime)
                    .num_seconds()
                    > POLLEN_INTERVAL
            }
            None => true,
        }
    }

    fn needs_uvi_refresh(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        match &maps.uvi {
            Some(uvi_maps) => {
                Utc::now()
                    .signed_duration_since(uvi_maps.mtime)
                    .num_seconds()
                    > UVI_INTERVAL
            }
            None => true,
        }
    }

    fn set_pollen(&self, retrieved_maps: Option<RetrievedMaps>) {
        if retrieved_maps.is_some() || self.is_pollen_stale() {
            let mut maps = self.lock().expect("Maps handle mutex was poisoned");
            maps.pollen = retrieved_maps;
        }
    }

    fn set_uvi(&self, retrieved_maps: Option<RetrievedMaps>) {
        if retrieved_maps.is_some() || self.is_uvi_stale() {
            let mut maps = self.lock().expect("Maps handle mutex was poisoned");
            maps.uvi = retrieved_maps;
        }
    }
}

/// A Buienradar map sample.
///
/// This represents a value at a given time.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(crate = "rocket::serde")]
pub(crate) struct Sample {
    /// The time(stamp) of the forecast.
    #[serde(serialize_with = "ts_seconds::serialize")]
    pub(crate) time: DateTime<Utc>,

    /// The forecasted score.
    ///
    /// A value in the range `1..=10`.
    #[serde(rename(serialize = "value"))]
    pub(crate) score: u8,
}

impl Sample {
    #[cfg(test)]
    pub(crate) fn new(time: DateTime<Utc>, score: u8) -> Self {
        Self { time, score }
    }
}

/// Builds a scoring histogram for the map key.
fn map_key_histogram() -> MapKeyHistogram {
    MAP_KEY
        .into_iter()
        .fold(HashMap::new(), |mut hm, channels| {
            hm.insert(Rgb::from(channels), 0);
            hm
        })
}

/// Samples the provided maps at the given (map-relative) coordinates and starting timestamp.
/// It assumes the provided coordinates are within bounds of at least one map.
/// The interval is the number of seconds the timestamp is bumped for each map.
///
/// Returns [`None`] if it encounters no known colors in any of the samples.
fn sample<I: GenericImageView<Pixel = Rgba<u8>>>(
    image: &I,
    stamp: DateTime<Utc>,
    interval: i64,
    count: u32,
    coords: (u32, u32),
) -> Option<Vec<Sample>> {
    let (x, y) = coords;
    let width = image.width() / count;
    let height = image.height();
    let max_sample_width = (width - x).min(MAP_SAMPLE_SIZE[0]);
    let max_sample_height = (height - y).min(MAP_SAMPLE_SIZE[1]);
    let mut samples = Vec::with_capacity(count as usize);
    let mut time = stamp;
    let mut offset = 0;

    while offset < image.width() {
        let map = image.view(
            x.saturating_sub(MAP_SAMPLE_SIZE[0] / 2) + offset,
            y.saturating_sub(MAP_SAMPLE_SIZE[1] / 2),
            max_sample_width,
            max_sample_height,
        );
        let histogram = map
            .pixels()
            .fold(map_key_histogram(), |mut h, (_px, _py, color)| {
                h.entry(color.to_rgb()).and_modify(|count| *count += 1);
                h
            });
        let (max_color, &count) = histogram
            .iter()
            .max_by_key(|(_color, count)| *count)
            .expect("Map key is never empty");
        if count == 0 {
            return None;
        }

        let score = MAP_KEY
            .iter()
            .position(|&color| &Rgb::from(color) == max_color)
            .map(|score| score + 1) // Scores go from 1..=10, not 0..=9!
            .expect("Maximum color is always a map key color") as u8;

        samples.push(Sample { time, score });
        time = time + chrono::Duration::seconds(interval as i64);
        offset += width;
    }

    Some(samples)
}

/// A retrieved image with some metadata.
#[derive(Debug)]
pub(crate) struct RetrievedMaps {
    /// The image data.
    pub(crate) image: DynamicImage,

    /// The date/time the image was last modified.
    pub(crate) mtime: DateTime<Utc>,

    /// The starting date/time the image corresponds with.
    pub(crate) timestamp_base: DateTime<Utc>,
}

impl RetrievedMaps {
    #[cfg(test)]
    pub(crate) fn new(image: DynamicImage) -> Self {
        let mtime = Utc::now();
        let timestamp_base = Utc::now();

        Self {
            image,
            mtime,
            timestamp_base,
        }
    }
}

/// Retrieves an image from the provided URL.
///
/// This returns [`None`] if it fails in either performing the request, parsing the `Last-Modified`
/// reponse HTTP header, retrieving the bytes from the image or loading and the decoding the data
/// into [`DynamicImage`].
async fn retrieve_image(url: Url) -> Option<RetrievedMaps> {
    // TODO: Handle or log errors!
    let response = reqwest::get(url).await.ok()?;
    let mtime = response
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|dt| dt.to_str().ok())
        .map(chrono::DateTime::parse_from_rfc2822)?
        .map(DateTime::<Utc>::from)
        .ok()?;
    let timestamp_base = {
        let path = response.url().path();
        let (_, filename) = path.rsplit_once('/')?;
        let (timestamp_str, _) = filename.split_once("__")?;
        let timestamp = NaiveDateTime::parse_from_str(timestamp_str, "%Y%m%d%H%M").ok()?;

        DateTime::<Utc>::from_utc(timestamp, Utc)
    };
    let bytes = response.bytes().await.ok()?;

    tokio::task::spawn_blocking(move || {
        if let Ok(image) = image::load_from_memory_with_format(&bytes, ImageFormat::Png) {
            Some(RetrievedMaps {
                image,
                mtime,
                timestamp_base,
            })
        } else {
            None
        }
    })
    .await
    .ok()?
}

/// Retrieves the pollen maps from Buienradar.
///
/// See [`POLLEN_BASE_URL`] for the base URL and [`retrieve_image`] for the retrieval function.
async fn retrieve_pollen_maps() -> Option<RetrievedMaps> {
    let timestamp = format!("{}", chrono::Local::now().format("%y%m%d%H%M"));
    let mut url = Url::parse(POLLEN_BASE_URL).unwrap();
    url.query_pairs_mut().append_pair("timestamp", &timestamp);

    println!("🔽 Refreshing pollen maps from: {}", url);
    retrieve_image(url).await
}

/// Retrieves the UV index maps from Buienradar.
///
/// See [`UVI_BASE_URL`] for the base URL and [`retrieve_image`] for the retrieval function.
async fn retrieve_uvi_maps() -> Option<RetrievedMaps> {
    let timestamp = format!("{}", chrono::Local::now().format("%y%m%d%H%M"));
    let mut url = Url::parse(UVI_BASE_URL).unwrap();
    url.query_pairs_mut().append_pair("timestamp", &timestamp);

    println!("🔽 Refreshing UV index maps from: {}", url);
    retrieve_image(url).await
}

/// Returns the map for the given instant.
///
/// This returns [`None`] if `instant` is too far in the future with respect to the number of
/// cached maps.
fn map_at(
    image: &DynamicImage,
    stamp: DateTime<Utc>,
    interval: i64,
    count: u32,
    instant: DateTime<Utc>,
) -> Option<DynamicImage> {
    let duration = instant.signed_duration_since(stamp);
    let offset = (duration.num_seconds() / interval) as u32;
    // Check if out of bounds.
    if offset >= count {
        return None;
    }
    let width = image.width() / count;

    Some(image.crop_imm(offset * width, 0, width, image.height()))
}

/// Marks the provided coordinates on the map using a horizontal and vertical line.
fn mark(mut image: DynamicImage, coords: (u32, u32)) -> DynamicImage {
    let (x, y) = coords;

    for py in 0..image.height() {
        image.put_pixel(x, py, Rgba::from([0x00, 0x00, 0x00, 0x70]));
    }
    for px in 0..image.width() {
        image.put_pixel(px, y, Rgba::from([0x00, 0x00, 0x00, 0x70]));
    }

    image
}

/// Projects the provided geocoded position to a coordinate on a map.
///
/// This uses two reference points and a Mercator projection on the y-coordinates of those points
/// to calculate how the map scales with respect to the provided position.
///
/// Returns [`None`] if the resulting coordinate is not within the bounds of the map.
fn project<I: GenericImageView>(
    image: &I,
    ref_points: [(Position, (u32, u32)); 2],
    pos: Position,
) -> Option<(u32, u32)> {
    // Get the data from the reference points.
    let (ref1, (ref1_y, ref1_x)) = ref_points[0];
    let (ref2, (ref2_y, ref2_x)) = ref_points[1];

    // For the x-coordinate, use a linear scale.
    let scale_x = ((ref2_x - ref1_x) as f64) / (ref2.lon_as_rad() - ref1.lon_as_rad());
    let x = ((pos.lon_as_rad() - ref1.lon_as_rad()) * scale_x + ref1_x as f64).round() as u32;

    // For the y-coordinate,  use a Mercator-projected scale.
    let mercator_y = |lat: f64| (lat / 2.0 + PI / 4.0).tan().ln();
    let ref1_merc_y = mercator_y(ref1.lat_as_rad());
    let ref2_merc_y = mercator_y(ref2.lat_as_rad());
    let scale_y = ((ref1_y - ref2_y) as f64) / (ref2_merc_y - ref1_merc_y);
    let y = ((ref2_merc_y - mercator_y(pos.lat_as_rad())) * scale_y + ref2_y as f64).round() as u32;

    if image.in_bounds(x, y) {
        Some((x, y))
    } else {
        None
    }
}

/// Returns the data of a map with a crosshair drawn on it for the given position.
///
/// The map that is used is determined by the provided metric.
pub(crate) async fn mark_map(
    position: Position,
    metric: Metric,
    maps_handle: &MapsHandle,
) -> Option<Vec<u8>> {
    use std::io::Cursor;

    let maps_handle = Arc::clone(maps_handle);
    tokio::task::spawn_blocking(move || {
        let maps = maps_handle.lock().expect("Maps handle lock was poisoned");
        let image = match metric {
            Metric::PAQI => maps.pollen_mark(position),
            Metric::Pollen => maps.pollen_mark(position),
            Metric::UVI => maps.uvi_mark(position),
            _ => return None, // Unsupported metric
        }?;
        drop(maps);

        // Encode the image as PNG image data.
        let mut image_data = Cursor::new(Vec::new());
        image
            .write_to(
                &mut image_data,
                image::ImageOutputFormat::from(image::ImageFormat::Png),
            )
            .ok()?;

        Some(image_data.into_inner())
    })
    .await
    .ok()
    .flatten()
}

/// Runs a loop that keeps refreshing the maps when necessary.
///
/// Use [`MapsRefresh`] trait methods on `maps_handle` to check whether each maps type needs to be
/// refreshed and uses its retrieval function to update it if necessary.
pub(crate) async fn run(maps_handle: MapsHandle) {
    loop {
        println!("🕔 Refreshing the maps (if necessary)...");

        if maps_handle.needs_pollen_refresh() {
            let retrieved_maps = retrieve_pollen_maps().await;
            maps_handle.set_pollen(retrieved_maps);
        }

        if maps_handle.needs_uvi_refresh() {
            let retrieved_maps = retrieve_uvi_maps().await;
            maps_handle.set_uvi(retrieved_maps);
        }

        sleep(REFRESH_INTERVAL).await;
    }
}
