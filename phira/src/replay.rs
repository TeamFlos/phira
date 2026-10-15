//! Local replays: the `.phrp` file format, and the per-chart store of saved
//! plays under `data/replays`.
//!
//! # File format
//!
//! | Field  | Encoding                   |
//! |--------|----------------------------|
//! | magic  | `b"PHRP"`                  |
//! | format | `u8`, currently [`FORMAT`] |
//! | body   | zstd(JSON(`ReplayFile`))   |
//!
//! The body is compressed at the same zstd level score uploads use. Readers
//! reject unknown formats, so an incompatible change can bump [`FORMAT`]
//! instead of misreading old files.
//!
//! # Store layout
//!
//! Each chart owns a directory named after its `local_path` — the first 16
//! hex chars of its SHA-256, so builtin, downloaded and custom charts all get
//! one. The player's own plays land there directly and are pruned by
//! [`save`] to the best score plus the [`RECENT_KEEP`] most recent; replays
//! imported from elsewhere live under `imported/` and are never pruned.

use crate::dir;
use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Utc};
use prpr::play_record::PlayRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tracing::warn;
use uuid::Uuid;

const MAGIC: &[u8; 4] = b"PHRP";
pub const FORMAT: u8 = 1;

/// Matches score uploads, which the records travel in too.
const ZSTD_LEVEL: i32 = 3;

/// How many plays a chart keeps besides its best score.
const RECENT_KEEP: usize = 5;

/// Everything about a replay except the record itself: what a list entry
/// shows, and what identifies the play when the file is shared.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayInfo {
    pub player_name: String,
    pub player_id: Option<i32>,
    pub played_at: DateTime<Utc>,
    pub score: u32,
    pub accuracy: f32,
    pub full_combo: bool,
    pub chart_name: String,
}

/// A `.phrp` file: the summary, and the record that replays the play.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayFile {
    pub info: ReplayInfo,
    pub record: PlayRecord,
}

/// One entry of a chart's replay list: a file on disk, what it holds, and
/// whether the player imported it.
#[derive(Clone, Debug)]
pub struct ReplayEntry {
    pub path: PathBuf,
    pub info: ReplayInfo,
    pub imported: bool,
}

pub fn encode(file: &ReplayFile) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(MAGIC);
    out.push(FORMAT);
    out.extend_from_slice(&zstd::encode_all(serde_json::to_vec(file)?.as_slice(), ZSTD_LEVEL)?);
    Ok(out)
}

pub fn decode(data: &[u8]) -> Result<ReplayFile> {
    if data.len() < 5 || &data[..4] != MAGIC {
        bail!("not a Phira replay");
    }
    let format = data[4];
    ensure!(format == FORMAT, "unsupported replay format {format}");
    let body = zstd::decode_all(&data[5..]).context("failed to decompress the replay")?;
    Ok(serde_json::from_slice(&body)?)
}

pub fn decode_file(path: &Path) -> Result<ReplayFile> {
    decode(&std::fs::read(path)?).with_context(|| format!("failed to read {}", path.display()))
}

/// The directory a chart's replays live in, keyed by a hash of its
/// `local_path` — any chart, builtin ones included, gets a stable name.
fn chart_dir(local_path: &str) -> Result<PathBuf> {
    use hex::ToHex;
    let hash: String = Sha256::digest(local_path.as_bytes()).encode_hex();
    Ok(Path::new(&dir::replays()?).join(&hash[..16]))
}

/// Writes one of the player's own plays, then prunes the chart's store.
pub fn save(local_path: &str, file: &ReplayFile) -> Result<()> {
    let dir = chart_dir(local_path)?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.phrp", file.info.played_at.timestamp_millis()));
    std::fs::write(&path, encode(file)?)?;
    if let Err(err) = cleanup(&dir) {
        // The play itself is saved; a leftover old replay costs disk, not data.
        warn!(?err, "failed to prune the chart's replays");
    }
    Ok(())
}

/// Writes a replay imported from elsewhere. It never takes part in pruning,
/// so importing cannot cost the player their own plays.
pub fn save_imported(local_path: &str, file: &ReplayFile) -> Result<PathBuf> {
    let dir = chart_dir(local_path)?.join("imported");
    std::fs::create_dir_all(&dir)?;
    let millis = file.info.played_at.timestamp_millis();
    let mut path = dir.join(format!("{millis}.phrp"));
    if path.exists() {
        // The same play imported twice within a millisecond still keeps
        // both copies.
        path = dir.join(format!("{millis}-{}.phrp", Uuid::new_v4()));
    }
    std::fs::write(&path, encode(file)?)?;
    Ok(path)
}

/// A chart's replays, newest first. An unreadable file is skipped with a
/// warning — one corrupt file must not hide the rest.
pub fn list(local_path: &str) -> Result<Vec<ReplayEntry>> {
    let dir = chart_dir(local_path)?;
    let mut entries = Vec::new();
    if dir.exists() {
        collect_dir(&dir, false, &mut entries)?;
    }
    let imported_dir = dir.join("imported");
    if imported_dir.exists() {
        collect_dir(&imported_dir, true, &mut entries)?;
    }
    entries.sort_by(|a, b| b.info.played_at.cmp(&a.info.played_at));
    Ok(entries)
}

fn collect_dir(dir: &Path, imported: bool, entries: &mut Vec<ReplayEntry>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|it| it != "phrp") {
            continue;
        }
        match decode_file(&path) {
            Ok(file) => entries.push(ReplayEntry {
                path,
                info: file.info,
                imported,
            }),
            Err(err) => warn!(?err, "skipping an unreadable replay"),
        }
    }
    Ok(())
}

/// Deletes one replay file.
pub fn delete(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Drops every replay of a chart, for when the chart itself is deleted.
pub fn remove_chart(local_path: &str) -> Result<()> {
    match std::fs::remove_dir_all(chart_dir(local_path)?) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Which of a chart's own replays survive a prune: the highest-scoring play
/// — the most recent one on a tie — plus the [`RECENT_KEEP`] most recent.
/// Pure, so the policy can be tested without a filesystem.
fn retained(entries: &[(u32, i64)]) -> Vec<bool> {
    let mut keep = vec![false; entries.len()];
    if let Some((best, _)) = entries.iter().enumerate().max_by_key(|(_, (score, time))| (*score, *time)) {
        keep[best] = true;
    }
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| (entries[i].1, i));
    for &index in order.iter().rev().take(RECENT_KEEP) {
        keep[index] = true;
    }
    keep
}

/// Applies [`retained`] to a chart's own replays; `imported/` is not part of
/// `dir`'s direct files, so imported ones are safe by construction.
fn cleanup(dir: &Path) -> Result<()> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|it| it != "phrp") {
            continue;
        }
        match decode_file(&path) {
            Ok(file) => files.push((file.info.score, file.info.played_at.timestamp_millis(), path)),
            Err(err) => warn!(?err, "skipping an unreadable replay"),
        }
    }
    let keys = files.iter().map(|(score, time, _)| (*score, *time)).collect::<Vec<_>>();
    for (keep, (_, _, path)) in retained(&keys).into_iter().zip(&files) {
        if !keep {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prpr::play_record::{PlayMeta, PLAY_RECORD_VERSION};

    fn file(score: u32, played_at_ms: i64) -> ReplayFile {
        ReplayFile {
            info: ReplayInfo {
                player_name: "player".into(),
                player_id: Some(42),
                played_at: DateTime::from_timestamp_millis(played_at_ms).unwrap(),
                score,
                accuracy: 0.99,
                full_combo: false,
                chart_name: "chart".into(),
            },
            record: PlayRecord {
                version: PLAY_RECORD_VERSION,
                meta: PlayMeta::default(),
                notes: Vec::new(),
                inputs: Vec::new(),
            },
        }
    }

    #[test]
    fn round_trips() {
        let file = file(1234567, 1_700_000_000_123);
        let data = encode(&file).unwrap();
        assert_eq!(&data[..4], b"PHRP");
        assert_eq!(data[4], FORMAT);
        let decoded = decode(&data).unwrap();
        assert_eq!(decoded.info, file.info);
        assert_eq!(decoded.record.version, PLAY_RECORD_VERSION);
    }

    #[test]
    fn rejects_a_bad_magic() {
        let mut data = encode(&file(1, 1)).unwrap();
        data[0] = b'X';
        assert!(decode(&data).is_err());
        assert!(decode(b"").is_err());
        assert!(decode(b"PHR").is_err());
        assert!(decode(b"PHRP").is_err());
    }

    #[test]
    fn rejects_an_unknown_format() {
        let mut data = encode(&file(1, 1)).unwrap();
        data[4] = FORMAT + 1;
        assert!(decode(&data).is_err());
    }

    #[test]
    fn rejects_truncated_data() {
        let data = encode(&file(1, 1)).unwrap();
        // One byte short of the whole body, and the bare header.
        assert!(decode(&data[..data.len() - 1]).is_err());
        assert!(decode(&data[..5]).is_err());
    }

    #[test]
    fn keeps_the_best_score_even_when_old() {
        // Plays with scores 0..7 at ascending times, plus a best score
        // played before all of them.
        let mut entries: Vec<(u32, i64)> = (0..8).map(|i| (i as u32, 10 + i as i64 * 10)).collect();
        entries.insert(0, (100, 0));
        let keep = retained(&entries);
        // The five most recent plays (times 40..80) plus the best at index 0;
        // the plays at times 10 and 30 are too old and too low to survive.
        assert_eq!(keep, vec![true, false, false, false, true, true, true, true, true], "{entries:?}");
    }

    #[test]
    fn keeps_only_five_when_the_best_is_recent() {
        let entries: Vec<(u32, i64)> = vec![(1, 0), (5, 10), (2, 20), (3, 30), (4, 40), (6, 50), (7, 60), (9, 70)];
        let keep = retained(&entries);
        // The best score (9) is the most recent play of all, so the union is
        // just the five most recent; the older and weaker ones go.
        assert_eq!(keep, vec![false, false, false, true, true, true, true, true], "{entries:?}");
    }

    #[test]
    fn breaks_score_ties_by_recency() {
        let entries: Vec<(u32, i64)> = vec![(5, 0), (5, 10), (1, 20), (2, 30), (3, 40), (4, 50), (1, 60), (1, 70)];
        let keep = retained(&entries);
        // Two plays share the best score; the later one is kept.
        assert_eq!(keep, vec![false, true, false, true, true, true, true, true], "{entries:?}");
    }

    #[test]
    fn keeps_everything_within_the_limit() {
        // Five plays fit the recent-five alone; a sixth would already be
        // pruned unless it were the best score.
        let entries: Vec<(u32, i64)> = (0..5).map(|i| (i as u32, i as i64)).collect();
        assert!(retained(&entries).into_iter().all(|it| it));
    }
}
