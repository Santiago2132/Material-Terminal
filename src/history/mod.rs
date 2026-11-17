//! Local SQLite command history persistence and hybrid SQLite + filesystem
//! ghost-text autocomplete engine.

use crate::config::default_data_dir;
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A persisted command execution record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: i64,
    pub command: String,
    pub working_dir: String,
    pub exit_code: Option<i32>,
    pub timestamp_ms: i64,
}

/// Manages local SQLite command history and fast prefix/path completions.
pub struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    /// Opens (or creates) the history SQLite database in the default data directory.
    pub fn open_default() -> Result<Self, String> {
        let dir = default_data_dir();
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create data directory {}: {e}", dir.display()))?;
        Self::open_at(&dir.join("history.sqlite"))
    }

    /// Opens (or creates) a SQLite history database at `db_path`.
    pub fn open_at(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create database dir {}: {e}", parent.display()))?;
        }
        let conn = Connection::open(db_path).map_err(|e| {
            format!(
                "Failed to open SQLite history at {}: {e}",
                db_path.display()
            )
        })?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Opens an in-memory SQLite store (useful for unit tests).
    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to open in-memory SQLite: {e}"))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
                CREATE TABLE IF NOT EXISTS history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    command TEXT NOT NULL,
                    working_dir TEXT NOT NULL,
                    exit_code INTEGER,
                    timestamp_ms INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_history_command ON history(command);
                ",
            )
            .map_err(|e| format!("Failed to initialize SQLite schema: {e}"))
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Records a submitted command and returns its row id.
    pub fn record_command(
        &self,
        command: &str,
        working_dir: &Path,
        exit_code: Option<i32>,
    ) -> Result<Option<i64>, String> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let cwd_str = working_dir.to_string_lossy();
        let ts = Self::now_ms();
        self.conn
            .execute(
                "INSERT INTO history (command, working_dir, exit_code, timestamp_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![trimmed, cwd_str.as_ref(), exit_code, ts],
            )
            .map_err(|e| format!("Failed to insert history command: {e}"))?;
        Ok(Some(self.conn.last_insert_rowid()))
    }

    /// Updates the exit code of a previously recorded command once it finishes.
    pub fn update_exit_code(&self, row_id: i64, exit_code: i32) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE history SET exit_code = ?1 WHERE id = ?2",
                params![exit_code, row_id],
            )
            .map_err(|e| format!("Failed to update history exit_code: {e}"))?;
        Ok(())
    }

    /// Returns deduplicated recent commands ordered most-recent-first.
    pub fn recent_unique(&self, limit: usize) -> Vec<HistoryEntry> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT h.id, h.command, h.working_dir, h.exit_code, h.timestamp_ms
             FROM history h
             INNER JOIN (
                 SELECT command, MAX(id) AS max_id
                 FROM history
                 GROUP BY command
             ) latest ON h.id = latest.max_id
             ORDER BY h.id DESC
             LIMIT ?1",
        ) else {
            return Vec::new();
        };

        let Ok(rows) = stmt.query_map(params![limit as i64], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                command: row.get(1)?,
                working_dir: row.get(2)?,
                exit_code: row.get(3)?,
                timestamp_ms: row.get(4)?,
            })
        }) else {
            return Vec::new();
        };

        rows.filter_map(Result::ok).collect()
    }

    /// Suggests the remaining ghost suffix for `input` using local paths first (when the
    /// active token looks like a path or follows a path command) and falling back to SQLite.
    pub fn suggest_ghost_suffix(&self, input: &str, working_dir: &Path) -> Option<String> {
        if input.is_empty() || input.contains('\n') {
            return None;
        }

        if let Some(path_suffix) = suggest_path_suffix(input, working_dir) {
            return Some(path_suffix);
        }

        // Fall back to most recent command starting with `input`.
        let pattern = format!("{}%", escape_like(input));
        let found: Option<String> = self
            .conn
            .query_row(
                "SELECT command FROM history
                 WHERE command LIKE ?1 ESCAPE '\\' AND command != ?2
                 ORDER BY id DESC
                 LIMIT 1",
                params![pattern, input],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();

        found.and_then(|cmd| cmd.strip_prefix(input).map(str::to_string))
    }
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn suggest_path_suffix(input: &str, working_dir: &Path) -> Option<String> {
    if input.ends_with(char::is_whitespace) {
        return None;
    }

    let (head, token) = match input.rfind(char::is_whitespace) {
        Some(idx) => (&input[..=idx], &input[idx + 1..]),
        None => ("", input),
    };

    if token.is_empty() {
        return None;
    }

    let first_word = head.split_whitespace().next().unwrap_or("");
    let path_command = matches!(
        first_word,
        "cd" | "ls"
            | "cat"
            | "bat"
            | "less"
            | "vim"
            | "nvim"
            | "nano"
            | "open"
            | "rm"
            | "cp"
            | "mv"
            | "mkdir"
            | "touch"
            | "head"
            | "tail"
    );
    let looks_like_path = token.starts_with("./")
        || token.starts_with("../")
        || token.starts_with('/')
        || token.starts_with("~/")
        || token.contains('/');

    if !looks_like_path && !path_command {
        return None;
    }

    let prefer_dirs = first_word == "cd";

    // Resolve the directory to scan and the filename prefix typed by the user.
    let expanded_token = if let Some(rest) = token.strip_prefix("~/") {
        dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(token))
    } else if token.starts_with('/') {
        PathBuf::from(token)
    } else {
        working_dir.join(token)
    };

    let (scan_dir, file_prefix) = if token.ends_with('/') {
        (expanded_token, String::new())
    } else {
        let parent = expanded_token
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| working_dir.to_path_buf());
        let prefix = expanded_token
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        (parent, prefix)
    };

    let entries = fs::read_dir(&scan_dir).ok()?;
    let mut candidates: Vec<(bool, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !file_prefix.starts_with('.') && name.starts_with('.') {
            continue;
        }
        if name.starts_with(&file_prefix) && name.len() > file_prefix.len() {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if prefer_dirs && !is_dir {
                continue;
            }
            candidates.push((is_dir, name));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let (is_dir, best_name) = candidates.into_iter().next()?;
    let mut suffix = best_name[file_prefix.len()..].to_string();
    if is_dir {
        suffix.push('/');
    }
    Some(suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sqlite_history_records_dedups_and_suggests_suffix() {
        let store = HistoryStore::open_in_memory().unwrap();
        let cwd = Path::new("/tmp");
        let id1 = store.record_command("cargo test --all", cwd, None).unwrap();
        assert!(id1.is_some());
        store.update_exit_code(id1.unwrap(), 0).unwrap();

        store
            .record_command("git status -sb", cwd, Some(0))
            .unwrap();
        store
            .record_command("cargo test --all", cwd, Some(0))
            .unwrap();

        let recent = store.recent_unique(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].command, "cargo test --all");
        assert_eq!(recent[0].exit_code, Some(0));
        assert_eq!(recent[1].command, "git status -sb");

        let suffix = store.suggest_ghost_suffix("cargo te", cwd);
        assert_eq!(suffix.as_deref(), Some("st --all"));
    }

    #[test]
    fn path_ghost_autocomplete_completes_local_directories_and_files() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mat-hist-path-{nanos}"));
        fs::create_dir_all(dir.join("projects_alpha")).unwrap();
        fs::write(dir.join("notes.txt"), "hello").unwrap();

        let store = HistoryStore::open_in_memory().unwrap();
        let dir_suffix = store.suggest_ghost_suffix("cd proj", &dir);
        assert_eq!(dir_suffix.as_deref(), Some("ects_alpha/"));

        let file_suffix = store.suggest_ghost_suffix("cat not", &dir);
        assert_eq!(file_suffix.as_deref(), Some("es.txt"));

        let _ = fs::remove_dir_all(dir);
    }
}
