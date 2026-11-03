use anyhow::Result;
use std::fs::File;
use std::path::Path;

use anyhow::Error;
use log::warn;
use std::thread::sleep;
use std::time::Duration;

pub(crate) fn open_file_task(path: &Path) -> Result<File> {
    const OPEN_FAIL_RETRY: usize = 3;
    const OPEN_RETRY_DELAY_MS: u64 = 100;
    let mut delay = Duration::from_millis(OPEN_RETRY_DELAY_MS);

    for attempt in 0..=OPEN_FAIL_RETRY {
        match File::open(path) {
            Ok(file) => return Ok(file),
            Err(e) if attempt < OPEN_FAIL_RETRY => {
                warn!(
                    "Attempt {}/{} failed to open {}: {e}. Retrying in {delay:?}…",
                    attempt + 1,
                    OPEN_FAIL_RETRY + 1,
                    path.display()
                );
                sleep(delay);
                delay = delay.checked_mul(2).unwrap_or(delay);
            }
            Err(e) => {
                return Err(Error::new(e).context(format!(
                    "Failed to open file {} after {} attempts",
                    path.display(),
                    OPEN_FAIL_RETRY + 1
                )));
            }
        }
    }

    unreachable!("open_file_with_retry logic error")
}
