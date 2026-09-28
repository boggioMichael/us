//! Where Syrup keeps what it learns, on this computer.
//!
//! ```text
//! <data>/games/<game>/profile.json     what Syrup learned about the game
//! <data>/games/<game>/knowledge.json   facts and the knowledge graph
//! <data>/games/<game>/episodes.json    moments worth remembering
//! <data>/players/<player>/model.json   the player model
//! <data>/sessions/<session>/...        one session's timeline and summary
//! <data>/recordings/                   frames, only when recording is on
//! ```
//!
//! Screenshots never go anywhere but `recordings/`, and only when asked.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    pub root: PathBuf,
}

impl DataDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        DataDir { root: root.into() }
    }

    /// `SYRUP_DATA_DIR` if set; otherwise the platform's place for app data.
    pub fn default_location() -> PathBuf {
        if let Some(dir) = std::env::var_os("SYRUP_DATA_DIR") {
            return PathBuf::from(dir);
        }
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        if cfg!(windows) {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                return PathBuf::from(appdata).join("SyrupUniversal");
            }
        } else if cfg!(target_os = "macos") {
            if let Some(home) = &home {
                return home.join("Library/Application Support/SyrupUniversal");
            }
        } else if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("syrup-universal");
        } else if let Some(home) = &home {
            return home.join(".local/share/syrup-universal");
        }
        PathBuf::from("syrup-data")
    }

    pub fn games(&self) -> PathBuf {
        self.root.join("games")
    }

    pub fn game(&self, game_id: &str) -> PathBuf {
        self.games().join(safe_name(game_id))
    }

    pub fn sessions(&self) -> PathBuf {
        self.root.join("sessions")
    }

    pub fn session(&self, session_id: &str) -> PathBuf {
        self.sessions().join(safe_name(session_id))
    }

    pub fn players(&self) -> PathBuf {
        self.root.join("players")
    }

    pub fn player(&self, player_id: &str) -> PathBuf {
        self.players().join(safe_name(player_id))
    }

    pub fn recordings(&self) -> PathBuf {
        self.root.join("recordings")
    }

    pub fn research_cache(&self) -> PathBuf {
        self.root.join("cache").join("research")
    }

    pub fn exists(&self) -> bool {
        Path::new(&self.root).exists()
    }
}

/// A name safe to use as one path component.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "_".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cannot_escape_the_data_directory() {
        assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_name("maplestory"), "maplestory");
        assert_eq!(safe_name(""), "_");
        let d = DataDir::new("/tmp/x");
        assert_eq!(d.game("a/b"), PathBuf::from("/tmp/x/games/a_b"));
    }
}
