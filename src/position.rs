//! Positions in the geographic coordinate system.
//!
//! This module contains everything related to geographic coordinate system functionality.

use std::f64::consts::PI;
use std::hash::Hash;

use cached::proc_macro::cached;
use geocoding::{Forward, Openstreetmap, Point};
use rocket::tokio;

use crate::{Error, Result};

/// A (geocoded) position.
///
/// This is used for measuring and communication positions directly on the Earth as latitude and
/// longitude.
///
/// # Position equivalence and hashing
///
/// For caching purposes we need to check equivalence between two positions. If the positions match
/// up to the 5th decimal, we consider them the same (see [`Position::lat_as_i32`] and
/// [`Position::lon_as_i32`]).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Position {
    /// The latitude of the position.
    pub(crate) lat: f64,

    /// The longitude of the position.
    pub(crate) lon: f64,
}

impl Position {
    /// Creates a new (geocoded) position.
    pub(crate) const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// Returns the latitude as an integer.
    ///
    /// This is achieved by multiplying it by `10_000` and rounding it.  Thus, this gives a
    /// precision of 5 decimals.
    fn lat_as_i32(&self) -> i32 {
        (self.lat * 10_000.0).round() as i32
    }

    /// Returns the longitude as an integer.
    ///
    /// This is achieved by multiplying it by `10_000` and rounding it.  Thus, this gives a
    /// precision of 5 decimals.
    fn lon_as_i32(&self) -> i32 {
        (self.lon * 10_000.0).round() as i32
    }

    /// Returns the latitude in radians.
    pub(crate) fn lat_as_rad(&self) -> f64 {
        self.lat * PI / 180.0
    }

    /// Returns the longitude in radians.
    pub(crate) fn lon_as_rad(&self) -> f64 {
        self.lon * PI / 180.0
    }

    /// Returns the latitude as a string with the given precision.
    pub(crate) fn lat_as_str(&self, precision: usize) -> String {
        format!("{:.*}", precision, self.lat)
    }

    /// Returns the longitude as a string with the given precision.
    pub(crate) fn lon_as_str(&self, precision: usize) -> String {
        format!("{:.*}", precision, self.lon)
    }
}

impl From<&Point<f64>> for Position {
    fn from(point: &Point<f64>) -> Self {
        // The `geocoding` API always returns (longitude, latitude) as (x, y).
        Position::new(point.y(), point.x())
    }
}

impl Hash for Position {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Floats cannot be hashed. Use the 5-decimal precision integer representation of the
        // coordinates instead.
        self.lat_as_i32().hash(state);
        self.lon_as_i32().hash(state);
    }
}

impl PartialEq for Position {
    fn eq(&self, other: &Self) -> bool {
        self.lat_as_i32() == other.lat_as_i32() && self.lon_as_i32() == other.lon_as_i32()
    }
}

impl Eq for Position {}

/// Resolves the geocoded position for a given address.
///
/// If the result is [`Ok`], it will be cached.
/// Note that only the 100 least recently used addresses will be cached.
#[cached(size = 100, result = true)]
pub(crate) async fn resolve_address(address: String) -> Result<Position> {
    println!("🌍 Geocoding the position of the address: {address}");
    tokio::task::spawn_blocking(move || {
        let osm = Openstreetmap::new();
        let points: Vec<Point<f64>> = osm.forward(&address)?;

        points
            .first()
            .ok_or(Error::NoPositionFound)
            .map(Position::from)
    })
    .await?
}
