use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn open(path: &PathBuf) -> Option<Arc<Mutex<File>>> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
        .map(Mutex::new)
        .map(Arc::new)
}

pub fn spawn_log_pump(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    path: PathBuf,
) {
    let log = match open(&path) {
        Some(l) => l,
        None => return,
    };

    if let Some(out) = stdout {
        let log = Arc::clone(&log);
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().flatten() {
                if let Ok(mut f) = log.lock() {
                    let _ = writeln!(f, "[out] {}", line);
                }
            }
        });
    }

    if let Some(err) = stderr {
        let log = Arc::clone(&log);
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().flatten() {
                if let Ok(mut f) = log.lock() {
                    let _ = writeln!(f, "[err] {}", line);
                }
            }
        });
    }
}
