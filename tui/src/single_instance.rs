//! Single-instance enforcement via a real OS file lock (`flock`).
//!
//! The lock file lives under `~/.cache/tui_game_station/` and is held with a
//! non-blocking exclusive `flock` (`File::try_lock`). The kernel releases the
//! lock the moment the owning file descriptor is closed — including when the
//! process dies abruptly (`kill -9`) — so a crashed instance never leaves a
//! "ghost" lock that would block later launches, and no PID file bookkeeping
//! is needed.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;

/// Lock file name used for the single-instance check.
pub const LOCK_FILE_NAME: &str = "tui_game_station.lock";

/// Try to acquire the single-instance lock.
///
/// Returns:
/// - `Ok(Some(file))` when THIS process got the lock. The caller must keep the
///   returned `File` alive for the whole app lifetime: dropping it releases
///   the lock (normal exit path).
/// - `Ok(None)` when another live instance already holds the lock.
/// - `Err(e)` when the lock file could not be opened/locked for any other
///   reason (permissions, filesystem errors). Callers may log a warning and
///   continue without the lock.
pub fn acquire_single_instance_lock() -> io::Result<Option<File>> {
    let path = lock_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

/// Where the single-instance lock file lives: `~/.cache/tui_game_station/`.
fn lock_file_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("tui_game_station")
        .join(LOCK_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_lock_path(tag: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "tui_game_station_test_{tag}_{}_{}.lock",
            std::process::id(),
            nonce
        ))
    }

    fn lock_file_at(path: &std::path::Path) -> File {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .expect("open lock file")
    }

    /// (a) Una segunda instancia debe rechazarse mientras la primera sostiene
    /// el lock; el `flock` no bloqueante devuelve `WouldBlock`.
    #[test]
    fn second_instance_is_rejected_while_first_holds_the_lock() {
        let path = temp_lock_path("double");
        let file1 = lock_file_at(&path);
        file1.try_lock().expect("first instance takes the lock");

        let file2 = lock_file_at(&path);
        let err = file2
            .try_lock()
            .expect_err("second try_lock must fail while the first holds it");
        assert!(
            matches!(err, std::fs::TryLockError::WouldBlock),
            "expected WouldBlock, got {err:?}"
        );

        drop(file1);
        drop(file2);
        let _ = std::fs::remove_file(&path);
    }

    /// (b) Un cierre abrupto (kill -9) cierra el fd y el kernel libera el
    /// flock automáticamente: una instancia nueva toma el lock sin quedar
    /// bloqueada por un lock huérfano.
    #[test]
    fn lock_is_released_when_the_holder_dies() {
        let path = temp_lock_path("stale");
        {
            // "Instancia A" toma el lock y "muere de golpe": al caer el File
            // (== cerrar el fd == el kernel suelta el flock), el archivo puede
            // seguir existiendo en disco pero ya no bloquea a nadie.
            let file_a = lock_file_at(&path);
            file_a.try_lock().expect("first instance takes the lock");
        }

        let file_b = lock_file_at(&path);
        file_b
            .try_lock()
            .expect("stale lock file must not block a new instance");
        drop(file_b);
        let _ = std::fs::remove_file(&path);
    }
}
