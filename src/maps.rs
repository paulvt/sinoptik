//! Maps retrieval and caching.
//!
//! This module provides a task that keeps maps up-to-date using a maps-specific refresh interval.
//! It stores all the maps as [`DynamicImage`]s in memory.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};

use chrono::serde::ts_seconds;
use chrono::{DateTime, Duration, Utc};
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
    [ 73, 218,  33],
    [ 48, 210,   0],
    [255, 248, 139],
    [255, 246,  66],
    [253, 187,  49],
    [253, 142,  36],
    [252,  16,  62],
    [150,  10,  51],
    [166, 109, 188],
    [179,  48, 161],
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
    fn set_pollen(&self, result: Option<(DynamicImage, DateTime<Utc>)>);

    /// Updates the UV index maps.
    fn set_uvi(&self, result: Option<(DynamicImage, DateTime<Utc>)>);
}

/// Container type for all in-memory cached maps.
#[derive(Debug)]
pub(crate) struct Maps {
    /// The pollen maps (from Buienradar).
    pub(crate) pollen: Option<DynamicImage>,

    /// The timestamp the pollen maps were last refreshed.
    pollen_stamp: DateTime<Utc>,

    /// The UV index maps (from Buienradar).
    pub(crate) uvi: Option<DynamicImage>,

    /// The timestamp the UV index maps were last refreshed.
    uvi_stamp: DateTime<Utc>,
}

impl Maps {
    /// Creates a new maps cache.
    ///
    /// It contains an [`DynamicImage`] per maps type, if downloaded, and the timestamp of the last
    /// update.
    pub(crate) fn new() -> Self {
        let now = Utc::now();
        Self {
            pollen: None,
            pollen_stamp: now,
            uvi: None,
            uvi_stamp: now,
        }
    }

    /// Returns a current pollen map that marks the provided position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet, there is no matching map for
    /// the current moment or if the provided position is not within the bounds of the map.
    pub(crate) fn pollen_mark(&self, position: Position) -> Option<DynamicImage> {
        self.pollen.as_ref().and_then(|maps| {
            let map = map_at(
                maps,
                self.pollen_stamp,
                POLLEN_MAP_INTERVAL,
                POLLEN_MAP_COUNT,
                Utc::now(),
            )?;
            let coords = project(&map, POLLEN_MAP_REF_POINTS, position)?;

            Some(mark(map, coords))
        })
    }

    /// Samples the pollen maps for the given position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    /// Otherwise, it returns [`Some`] with a list of pollen sample, one for each map
    /// in the series of maps.
    pub(crate) fn pollen_samples(&self, position: Position) -> Option<Vec<Sample>> {
        self.pollen.as_ref().and_then(|maps| {
            let map = maps.view(0, 0, maps.width() / UVI_MAP_COUNT, maps.height());
            let coords = project(&*map, POLLEN_MAP_REF_POINTS, position)?;

            sample(
                maps,
                self.pollen_stamp,
                POLLEN_MAP_INTERVAL,
                POLLEN_MAP_COUNT,
                coords,
            )
        })
    }

    /// Returns a current UV index map that marks the provided position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet, there is no matching map for
    /// the current moment or if the provided position is not within the bounds of the map.
    pub(crate) fn uvi_mark(&self, position: Position) -> Option<DynamicImage> {
        self.uvi.as_ref().and_then(|maps| {
            let map = map_at(
                maps,
                self.uvi_stamp,
                UVI_MAP_INTERVAL,
                UVI_MAP_COUNT,
                Utc::now(),
            )?;
            let coords = project(&map, POLLEN_MAP_REF_POINTS, position)?;

            Some(mark(map, coords))
        })
    }

    /// Samples the UV index maps for the given position.
    ///
    /// This returns [`None`] if the maps are not in the cache yet.
    /// Otherwise, it returns [`Some`] with a list of UV index sample, one for each map
    /// in the series of maps.
    pub(crate) fn uvi_samples(&self, position: Position) -> Option<Vec<Sample>> {
        self.uvi.as_ref().and_then(|maps| {
            let map = maps.view(0, 0, maps.width() / UVI_MAP_COUNT, maps.height());
            let coords = project(&*map, UVI_MAP_REF_POINTS, position)?;

            sample(
                maps,
                self.uvi_stamp,
                UVI_MAP_INTERVAL,
                UVI_MAP_COUNT,
                coords,
            )
        })
    }
}

impl MapsRefresh for MapsHandle {
    fn is_pollen_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        Utc::now().signed_duration_since(maps.pollen_stamp)
            > Duration::seconds(POLLEN_MAP_COUNT as i64 * POLLEN_MAP_INTERVAL)
    }

    fn is_uvi_stale(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        Utc::now().signed_duration_since(maps.uvi_stamp)
            > Duration::seconds(UVI_MAP_COUNT as i64 * UVI_MAP_INTERVAL)
    }

    fn needs_pollen_refresh(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        maps.pollen.is_none()
            || Utc::now()
                .signed_duration_since(maps.pollen_stamp)
                .num_seconds()
                > POLLEN_INTERVAL
    }

    fn needs_uvi_refresh(&self) -> bool {
        let maps = self.lock().expect("Maps handle mutex was poisoned");

        maps.uvi.is_none()
            || Utc::now()
                .signed_duration_since(maps.uvi_stamp)
                .num_seconds()
                > UVI_INTERVAL
    }

    fn set_pollen(&self, result: Option<(DynamicImage, DateTime<Utc>)>) {
        if result.is_some() || self.is_pollen_stale() {
            let mut maps = self.lock().expect("Maps handle mutex was poisoned");

            if let Some((pollen, pollen_stamp)) = result {
                maps.pollen = Some(pollen);
                maps.pollen_stamp = pollen_stamp
            } else {
                maps.pollen = None
            }
        }
    }

    fn set_uvi(&self, result: Option<(DynamicImage, DateTime<Utc>)>) {
        if result.is_some() || self.is_uvi_stale() {
            let mut maps = self.lock().expect("Maps handle mutex was poisoned");

            if let Some((uvi, uvi_stamp)) = result {
                maps.uvi = Some(uvi);
                maps.uvi_stamp = uvi_stamp
            } else {
                maps.uvi = None
            }
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
    maps: &I,
    stamp: DateTime<Utc>,
    interval: i64,
    count: u32,
    coords: (u32, u32),
) -> Option<Vec<Sample>> {
    let (x, y) = coords;
    let width = maps.width() / count;
    let height = maps.height();
    let max_sample_width = (width - x).min(MAP_SAMPLE_SIZE[0]);
    let max_sample_height = (height - y).min(MAP_SAMPLE_SIZE[1]);
    let mut samples = Vec::with_capacity(count as usize);
    let mut time = stamp;
    let mut offset = 0;

    while offset < maps.width() {
        let map = maps.view(
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

/// Retrieves an image from the provided URL.
///
/// This returns [`None`] if it fails in either performing the request, parsing the `Last-Modified`
/// reponse HTTP header, retrieving the bytes from the image or loading and the decoding the data
/// into [`DynamicImage`].
async fn retrieve_image(url: Url) -> Option<(DynamicImage, DateTime<Utc>)> {
    // TODO: Handle or log errors!
    let response = reqwest::get(url).await.ok()?;
    let mtime = response
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|dt| dt.to_str().ok())
        .map(chrono::DateTime::parse_from_rfc2822)?
        .map(DateTime::<Utc>::from)
        .ok()?;
    let bytes = response.bytes().await.ok()?;

    tokio::task::spawn_blocking(move || {
        if let Ok(image) = image::load_from_memory_with_format(&bytes, ImageFormat::Png) {
            Some((image, mtime))
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
async fn retrieve_pollen_maps() -> Option<(DynamicImage, DateTime<Utc>)> {
    let timestamp = format!("{}", chrono::Local::now().format("%y%m%d%H%M"));
    let mut url = Url::parse(POLLEN_BASE_URL).unwrap();
    url.query_pairs_mut().append_pair("timestamp", &timestamp);

    println!("🔽 Refreshing pollen maps from: {}", url);
    retrieve_image(url).await
}

/// Retrieves the UV index maps from Buienradar.
///
/// See [`UVI_BASE_URL`] for the base URL and [`retrieve_image`] for the retrieval function.
async fn retrieve_uvi_maps() -> Option<(DynamicImage, DateTime<Utc>)> {
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
    maps: &DynamicImage,
    maps_stamp: DateTime<Utc>,
    interval: i64,
    count: u32,
    instant: DateTime<Utc>,
) -> Option<DynamicImage> {
    let duration = instant.signed_duration_since(maps_stamp);
    let offset = (duration.num_seconds() / interval) as u32;
    // Check if out of bounds.
    if offset >= count {
        return None;
    }
    let width = maps.width() / count;

    Some(maps.crop_imm(offset * width, 0, width, maps.height()))
}

/// Marks the provided coordinates on the map using a horizontal and vertical line.
fn mark(mut map: DynamicImage, coords: (u32, u32)) -> DynamicImage {
    let (x, y) = coords;

    for py in 0..map.height() {
        map.put_pixel(x, py, Rgba::from([0x00, 0x00, 0x00, 0x70]));
    }
    for px in 0..map.width() {
        map.put_pixel(px, y, Rgba::from([0x00, 0x00, 0x00, 0x70]));
    }

    map
}

/// Projects the provided geocoded position to a coordinate on a map.
///
/// This uses two reference points and a Mercator projection on the y-coordinates of those points
/// to calculate how the map scales with respect to the provided position.
///
/// Returns [`None`] if the resulting coordinate is not within the bounds of the map.
fn project<I: GenericImageView>(
    map: &I,
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

    if map.in_bounds(x, y) {
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
            let result = retrieve_pollen_maps().await;
            maps_handle.set_pollen(result);
        }

        if maps_handle.needs_uvi_refresh() {
            let result = retrieve_uvi_maps().await;
            maps_handle.set_uvi(result);
        }

        sleep(REFRESH_INTERVAL).await;
    }
}
