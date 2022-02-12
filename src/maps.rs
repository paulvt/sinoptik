//! Maps handling.

use rocket::tokio::time::{sleep, Duration};

use crate::MAPS;

/// The interval between map refreshes (in seconds).
const SLEEP_INTERVAL: u64 = 60;

#[derive(Debug, Default)]
pub(crate) struct Maps;

impl Maps {
    pub(crate) async fn run() -> ! {
        loop {
            let _maps = MAPS.lock().await;

            sleep(Duration::from_secs(SLEEP_INTERVAL)).await;
        }
    }
}
