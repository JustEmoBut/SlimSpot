//! Writes warnings and errors (ours and librespot's) to `slimspot.log` in the cache dir.
//! The app has no console (windows_subsystem), so without this librespot's diagnostics vanish.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

const LOG_FILE: &str = "slimspot.log";
// Start fresh once the file passes this; only warnings/errors are written, so it grows slowly.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

struct FileLogger(Mutex<File>);

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Warn
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if let Ok(mut file) = self.0.lock() {
            // Logging must never take the app down; a failed write is dropped.
            let _ = writeln!(file, "{secs} {} {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}

/// Best effort: if the file can't be opened the app simply runs without a log.
pub fn init(dir: &Path) {
    let path = dir.join(LOG_FILE);
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    let file = std::fs::create_dir_all(dir).and_then(|()| {
        OpenOptions::new().create(true).append(!too_big).write(true).truncate(too_big).open(&path)
    });
    if let Ok(file) = file {
        if log::set_boxed_logger(Box::new(FileLogger(Mutex::new(file)))).is_ok() {
            log::set_max_level(LevelFilter::Warn);
        }
    }
}
