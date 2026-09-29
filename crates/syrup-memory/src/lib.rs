//! What Syrup remembers, on this computer, as plain JSON files.
//!
//! | layer | what | where |
//! |---|---|---|
//! | game | the profile (what Syrup learned about the game) | `games/<game>/profile.json` |
//! | knowledge | facts and the knowledge graph | `games/<game>/knowledge.json` |
//! | episodes | moments worth remembering | `games/<game>/episodes.json` |
//! | player | the player model | `players/<player>/model.json` |
//! | session | one session's timeline and summary | `sessions/<session>/timeline.jsonl`, `summary.json` |
//! | recordings | frames, only when recording is on | `recordings/<session>/` |
//!
//! Learned metadata and pictures never mix: frames are only written under
//! `recordings/`, and only when asked. Everything can be forgotten: one game,
//! the player, the recordings, or all of it.

pub mod learner;

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use syrup_core::paths::{DataDir, safe_name};
use syrup_core::{EventRecord, GameProfile};

pub use learner::ProfileLearner;

/// Writes `value` as JSON atomically (a temporary file, then a rename), so a
/// crash never leaves half a profile behind.
pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = BufWriter::new(File::create(&tmp)?);
        serde_json::to_writer_pretty(&mut f, value).map_err(io::Error::other)?;
        f.flush()?;
    }
    fs::rename(&tmp, path)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The data directory, and everything read from and written to it.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    pub dir: DataDir,
}

impl MemoryStore {
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = DataDir::new(root);
        fs::create_dir_all(&dir.root)?;
        Ok(MemoryStore { dir })
    }

    pub fn default_location() -> io::Result<Self> {
        MemoryStore::open(DataDir::default_location())
    }

    pub fn root(&self) -> &Path {
        &self.dir.root
    }

    pub fn game_file(&self, game_id: &str, name: &str) -> PathBuf {
        self.dir.game(game_id).join(name)
    }

    pub fn player_file(&self, player: &str, name: &str) -> PathBuf {
        self.dir.player(player).join(name)
    }

    pub fn load_profile(&self, game_id: &str) -> Option<GameProfile> {
        read_json(&self.game_file(game_id, "profile.json"))
    }

    pub fn save_profile(&self, p: &GameProfile) -> io::Result<()> {
        write_json(&self.game_file(&p.game_id, "profile.json"), p)
    }

    /// Every profile on disk.
    pub fn profiles(&self) -> Vec<GameProfile> {
        let Ok(entries) = fs::read_dir(self.dir.games()) else { return Vec::new() };
        let mut out: Vec<GameProfile> =
            entries.filter_map(|e| e.ok()).filter_map(|e| read_json(&e.path().join("profile.json"))).collect();
        out.sort_by(|a, b| a.game_id.cmp(&b.game_id));
        out
    }

    pub fn load<T: DeserializeOwned>(&self, game_id: &str, name: &str) -> Option<T> {
        read_json(&self.game_file(game_id, name))
    }

    pub fn save<T: Serialize>(&self, game_id: &str, name: &str, value: &T) -> io::Result<()> {
        write_json(&self.game_file(game_id, name), value)
    }

    pub fn load_player<T: DeserializeOwned>(&self, player: &str) -> Option<T> {
        read_json(&self.player_file(player, "model.json"))
    }

    pub fn save_player<T: Serialize>(&self, player: &str, value: &T) -> io::Result<()> {
        write_json(&self.player_file(player, "model.json"), value)
    }

    pub fn episodes(&self, game_id: &str) -> Vec<Episode> {
        self.load(game_id, "episodes.json").unwrap_or_default()
    }

    /// Adds a moment to the game's episodes (the oldest go past 500).
    pub fn add_episode(&self, game_id: &str, e: Episode) -> io::Result<()> {
        let mut all = self.episodes(game_id);
        all.push(e);
        if all.len() > 500 {
            let extra = all.len() - 500;
            all.drain(..extra);
        }
        self.save(game_id, "episodes.json", &all)
    }

    /// Forgets one game entirely: profile, knowledge, episodes.
    pub fn forget_game(&self, game_id: &str) -> io::Result<bool> {
        remove_dir(&self.dir.game(game_id))
    }

    /// Forgets the player: the player model.
    pub fn forget_player(&self, player: &str) -> io::Result<bool> {
        remove_dir(&self.dir.player(player))
    }

    /// Deletes every recorded frame.
    pub fn forget_recordings(&self) -> io::Result<bool> {
        remove_dir(&self.dir.recordings())
    }

    /// Deletes session timelines.
    pub fn forget_sessions(&self) -> io::Result<bool> {
        remove_dir(&self.dir.sessions())
    }

    /// Deletes everything Syrup has stored.
    pub fn forget_everything(&self) -> io::Result<()> {
        for d in [
            self.dir.games(),
            self.dir.players(),
            self.dir.sessions(),
            self.dir.recordings(),
            self.dir.research_cache(),
        ] {
            remove_dir(&d)?;
        }
        Ok(())
    }

    /// A folder for this session's recorded frames (only used when recording is on).
    pub fn recording_dir(&self, session: &str) -> PathBuf {
        self.dir.recordings().join(safe_name(session))
    }
}

fn remove_dir(p: &Path) -> io::Result<bool> {
    match fs::remove_dir_all(p) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// A moment worth remembering: a death, a comeback, a first win.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    /// When, ISO 8601.
    pub at: String,
    pub session: String,
    /// `death`, `victory`, `comeback`, `level_up`, `boss`...
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub details: std::collections::BTreeMap<String, String>,
}

/// How a session went, written when it ends.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session: String,
    pub game_id: Option<String>,
    pub title: Option<String>,
    pub started: String,
    pub ended: String,
    pub duration_s: f64,
    pub analysed_frames: u64,
    pub deaths: u32,
    pub victories: u32,
    pub level_ups: u32,
    pub advice_shown: u32,
    pub advice_suppressed: u32,
    pub feedback: std::collections::BTreeMap<String, u32>,
    pub concepts: Vec<String>,
    pub highlights: Vec<String>,
}

/// The session's timeline: every event, one JSON object per line.
pub struct SessionLog {
    pub id: String,
    pub dir: PathBuf,
    out: BufWriter<File>,
}

impl SessionLog {
    pub fn create(store: &MemoryStore, id: &str) -> io::Result<Self> {
        let dir = store.dir.session(id);
        fs::create_dir_all(&dir)?;
        let out = BufWriter::new(File::create(dir.join("timeline.jsonl"))?);
        Ok(SessionLog { id: id.to_string(), dir, out })
    }

    pub fn write(&mut self, record: &EventRecord) {
        if let Ok(line) = serde_json::to_string(record) {
            let _ = writeln!(self.out, "{line}");
        }
    }

    pub fn flush(&mut self) {
        let _ = self.out.flush();
    }

    pub fn finish(mut self, summary: &SessionSummary) -> io::Result<PathBuf> {
        self.out.flush()?;
        let path = self.dir.join("summary.json");
        write_json(&path, summary)?;
        Ok(path)
    }
}

/// Reads a session's timeline back.
pub fn read_timeline(path: &Path) -> Vec<EventRecord> {
    fs::read_to_string(path)
        .map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

/// A new session id from the time: `20260929-013000`.
pub fn session_id(now_unix: u64) -> String {
    let iso = syrup_core::util::iso_from_unix(now_unix);
    format!("{}-{}", iso[0..10].replace('-', ""), iso[11..19].replace(':', ""))
}

impl MemoryStore {
    /// A session id no earlier session in this store has used (two sessions
    /// can start in the same second: `20260929-013000-2`).
    pub fn new_session_id(&self, now_unix: u64) -> String {
        let base = session_id(now_unix);
        let mut id = base.clone();
        let mut n = 2;
        while self.dir.session(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }
}

#[cfg(test)]
mod tests {
    use syrup_core::Event;

    use super::*;

    #[test]
    fn profiles_round_trip_and_can_be_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let mut p = GameProfile::new("elden-ring", "ELDEN RING");
        p.genres.push("soulslike".into());
        store.save_profile(&p).unwrap();
        assert_eq!(store.load_profile("elden-ring"), Some(p.clone()));
        assert_eq!(store.profiles().len(), 1);
        store
            .add_episode(
                "elden-ring",
                Episode {
                    at: "2026-09-29T01:00:00Z".into(),
                    session: "s".into(),
                    kind: "death".into(),
                    summary: "died to a boss".into(),
                    details: Default::default(),
                },
            )
            .unwrap();
        assert_eq!(store.episodes("elden-ring").len(), 1);
        assert!(store.forget_game("elden-ring").unwrap());
        assert!(store.load_profile("elden-ring").is_none());
        assert!(!store.forget_game("elden-ring").unwrap());
    }

    #[test]
    fn a_session_timeline_is_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let mut log = SessionLog::create(&store, &session_id(1_790_631_500)).unwrap();
        assert_eq!(log.id, "20260928-213820");
        log.write(&EventRecord { seq: 1, event: Event::Note { ts_ms: 5, message: "hello".into() } });
        let path = log.dir.join("timeline.jsonl");
        let summary = log.finish(&SessionSummary { deaths: 2, ..Default::default() }).unwrap();
        assert_eq!(read_timeline(&path).len(), 1);
        let s: SessionSummary = read_json(&summary).unwrap();
        assert_eq!(s.deaths, 2);
        store.forget_everything().unwrap();
        assert!(!store.dir.sessions().exists());
    }
}
