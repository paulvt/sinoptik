#![doc = include_str!("../README.md")]
#![warn(
    clippy::all,
    missing_debug_implementations,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links
)]
#![deny(missing_docs)]

use color_eyre::Result;
use rocket::tokio::{self, select};

/// Starts the main maps refresh task and sets up and launches Rocket.
#[rocket::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let (rocket, maps_refresher) = sinoptik::setup();
    let rocket = rocket.ignite().await?;
    let shutdown = rocket.shutdown();
    let maps_refresher = tokio::spawn(maps_refresher);

    select! {
        result = rocket.launch() => {
            result?
        }
        result = maps_refresher => {
            shutdown.notify();
            result?
        }
    }

    Ok(())
}
