//! Replay recording and playback data.
//!
//! A replay captures, in time order, every per-note judgement made during a
//! play. During playback the engine consumes these records and applies the
//! same judgements to the matching notes without user input.

use crate::judge::Judgement;
use serde::{Deserialize, Serialize};

/// A single judgement event in a replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteRecord {
    /// Game time (in seconds, relative to chart start) at which this judgement
    /// was committed.
    pub time: f64,
    /// Index of the note's line (0-based, matches `Chart::lines`).
    pub line_id: u32,
    /// Index of the note within the line (matches `Line::notes`).
    pub note_id: u32,
    /// The committed judgement.
    pub judgment: ReplayJudgement,
    /// Signed offset in seconds. Negative = early, positive = late. For
    /// `Perfect`/`Good`/`Bad` this is the raw press - note.time; for `Miss`
    /// and hold pre-judges it is 0.
    pub offset: f64,
}

/// Persistable form of `Judgement` including hold pre/end variants we care
/// about for accurate playback.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplayJudgement {
    Perfect,
    Good,
    Bad,
    Miss,
    /// Hold note accepted on press (no scoring effect yet).
    HoldPerfect,
    /// Hold note accepted on press, rated Good.
    HoldGood,
}

impl ReplayJudgement {
    pub fn from_commit(r: Result<Judgement, bool>) -> Self {
        match r {
            Ok(Judgement::Perfect) => Self::Perfect,
            Ok(Judgement::Good) => Self::Good,
            Ok(Judgement::Bad) => Self::Bad,
            Ok(Judgement::Miss) => Self::Miss,
            Err(true) => Self::HoldPerfect,
            Err(false) => Self::HoldGood,
        }
    }

    /// Returns the visual `Judgement` to commit, or `None` if this record
    /// represents only a hold pre-judge (which scores nothing on its own).
    pub fn to_judgement(self) -> Option<Judgement> {
        match self {
            Self::Perfect => Some(Judgement::Perfect),
            Self::Good => Some(Judgement::Good),
            Self::Bad => Some(Judgement::Bad),
            Self::Miss => Some(Judgement::Miss),
            Self::HoldPerfect | Self::HoldGood => None,
        }
    }

    pub fn is_hold_prejudge(self) -> bool {
        matches!(self, Self::HoldPerfect | Self::HoldGood)
    }
}

/// A full replay tied to a specific chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayData {
    /// Phira chart id when the replay was recorded from an online chart.
    pub chart_id: Option<i32>,
    /// Display name of the chart (used for matching when id is missing).
    pub chart_name: String,
    /// Difficulty level of the chart at recording time (e.g. "IN Lv.13").
    #[serde(default)]
    pub chart_level: String,
    /// Chart offset (seconds) when the replay was recorded.
    #[serde(default)]
    pub chart_offset: f32,
    /// Playback speed at recording time.
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Chronological list of per-note judgement events.
    pub records: Vec<NoteRecord>,
    /// Final score (0..=1_000_000).
    #[serde(default)]
    pub score: i32,
    /// Final accuracy (0.0..=1.0).
    #[serde(default)]
    pub accuracy: f32,
    /// Max combo reached.
    #[serde(default)]
    pub max_combo: u32,
    /// Full-combo flag.
    #[serde(default)]
    pub full_combo: bool,
    /// Unix timestamp of the game session.
    pub timestamp: i64,
}

fn default_speed() -> f32 {
    1.0
}

impl ReplayData {
    pub fn new(chart_id: Option<i32>, chart_name: String) -> Self {
        Self {
            chart_id,
            chart_name,
            chart_level: String::new(),
            chart_offset: 0.,
            speed: 1.,
            records: Vec::new(),
            score: 0,
            accuracy: 0.,
            max_combo: 0,
            full_combo: false,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    pub fn finalize(&mut self, score: i32, accuracy: f32, max_combo: u32, full_combo: bool) {
        self.score = score;
        self.accuracy = accuracy;
        self.max_combo = max_combo;
        self.full_combo = full_combo;
    }
}

// ------- replay file storage -------

use std::path::{Path, PathBuf};

fn replay_dir() -> anyhow::Result<PathBuf> {
    let root = crate::get_data_dir().ok_or_else(|| anyhow::anyhow!("data dir not set"))?;
    let dir = Path::new(&root).join("replays");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Serialize `data` as pretty JSON into the replay directory. The filename
/// encodes timestamp and chart name to avoid collisions.
pub fn save_replay_file(data: &ReplayData) -> anyhow::Result<PathBuf> {
    let dir = replay_dir()?;
    let safe_name: String = data
        .chart_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let filename = format!("{}_{}.json", data.timestamp, safe_name);
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

// --- thread-local handoff so callers can queue a replay/recording config
//     for the next-built GameScene without modifying its public API. ---

use std::cell::RefCell;

thread_local! {
    /// A pending `ReplayData` that the next `GameScene` constructed on this
    /// thread should play back (rather than recording).
    static PENDING_PLAYBACK: RefCell<Option<ReplayData>> = const { RefCell::new(None) };

    /// A pending recording-start signal: when this is `true` the next
    /// `GameScene::new` will start recording into a fresh `ReplayData`.
    /// Set by phira's launch path after consulting `auto_record`.
    static PENDING_RECORD: RefCell<bool> = const { RefCell::new(false) };
}

/// Queue replay data to be played back by the next `GameScene` on this thread.
pub fn set_pending_playback(data: ReplayData) {
    PENDING_PLAYBACK.with(|cell| *cell.borrow_mut() = Some(data));
}

pub fn take_pending_playback() -> Option<ReplayData> {
    PENDING_PLAYBACK.with(|cell| cell.borrow_mut().take())
}

/// Queue the next built `GameScene` to start recording a replay.
pub fn set_pending_record(on: bool) {
    PENDING_RECORD.with(|cell| *cell.borrow_mut() = on);
}

pub fn take_pending_record() -> bool {
    PENDING_RECORD.with(|cell| {
        let v = *cell.borrow();
        *cell.borrow_mut() = false;
        v
    })
}
