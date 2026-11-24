//! Privacy-safe, bounded local desktop host logging.
//!
//! Records shell lifecycle, directory resolution, and sanitized error events
//! without capturing user images, masks, prompts, or personal file paths.
use super::paths;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const DEFAULT_MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const LOG_FILE_NAME: &str = "sculpt.log";
const PREV_LOG_FILE_NAME: &str = "sculpt.prev.log";

pub struct HostLogger {
    log_path: Option<PathBuf>,
    guard: Mutex<()>,
    max_bytes: u64,
}

impl HostLogger {
    pub fn new(dir: Result<PathBuf, String>) -> Self {
        Self::with_limit(dir, DEFAULT_MAX_LOG_BYTES)
    }

    pub fn with_limit(dir: Result<PathBuf, String>, max_bytes: u64) -> Self {
        let log_path = dir.ok().and_then(|root| {
            fs::create_dir_all(&root).ok()?;
            Some(root.join(LOG_FILE_NAME))
        });
        Self {
            log_path,
            guard: Mutex::new(()),
            max_bytes: max_bytes.max(1024),
        }
    }

    pub fn log(&self, level: &str, component: &str, message: &str) {
        let Some(path) = &self.log_path else {
            return;
        };
        let Ok(_lock) = self.guard.lock() else {
            return;
        };

        if let Ok(meta) = fs::metadata(path) {
            if meta.len() >= self.max_bytes {
                if let Some(parent) = path.parent() {
                    let prev = parent.join(PREV_LOG_FILE_NAME);
                    let _ = paths::atomic_replace(path, &prev);
                }
            }
        }

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let line = format!(
            "[{timestamp_ms}] [{}] [{}] {}\n",
            sanitize_log_text(level),
            sanitize_log_text(component),
            sanitize_log_text(message),
        );

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }

    pub fn info(&self, component: &str, message: &str) {
        self.log("INFO", component, message);
    }

    #[allow(dead_code)]
    pub fn warn(&self, component: &str, message: &str) {
        self.log("WARN", component, message);
    }

    pub fn error(&self, component: &str, message: &str) {
        self.log("ERROR", component, message);
    }

    #[allow(dead_code)]
    pub fn path(&self) -> Option<&PathBuf> {
        self.log_path.as_ref()
    }
}

pub fn sanitize_log_text(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_rotates_bounded_local_logs() {
        let root = std::env::temp_dir().join(format!("sculpt-log-test-{}", uuid::Uuid::new_v4()));
        let logger = HostLogger::with_limit(Ok(root.clone()), 1024);

        for i in 0..40 {
            logger.info(
                "shell",
                &format!("startup check iteration {i} padding-payload-0123456789abcdef"),
            );
        }

        let current = root.join(LOG_FILE_NAME);
        let prev = root.join(PREV_LOG_FILE_NAME);
        assert!(current.is_file(), "current log file should exist");
        assert!(prev.is_file(), "rotated backup log file should exist");
        let content = fs::read_to_string(&current).unwrap();
        assert!(content.contains("[INFO] [shell]"));
        assert!(!content.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sanitizes_control_characters_and_handles_unavailable_directory() {
        assert_eq!(
            sanitize_log_text("line1\n[ERROR] forged\r\tline2"),
            "line1 [ERROR] forged  line2"
        );

        let blocked = HostLogger::new(Err("no log directory".into()));
        assert!(blocked.path().is_none());
        blocked.warn("shell", "this should not panic");
        blocked.error("library", "this should not panic");
    }
}
