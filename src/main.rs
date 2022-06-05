#![doc = include_str!("../README.md")]
#![warn(
    clippy::all,
    missing_debug_implementations,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links
)]
#![deny(missing_docs)]

/// Starts the main maps refresh task and sets up and launches Rocket.
#[rocket::launch]
async fn rocket() -> _ {
    sinoptik::setup()
}
