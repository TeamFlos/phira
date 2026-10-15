//! Replay support types, shared by the game and ending scenes.

use crate::{judge::PlayResult, play_record::PlayRecord};
use anyhow::Result;
use std::sync::Arc;

/// A replay to play: the record itself, and whose play it was.
pub struct ReplayRequest {
    pub record: Arc<PlayRecord>,
    pub player_name: String,
}

/// Persists a finished play's record locally. Called once per recorded run,
/// where the game scene ends; a failure is worth a warning, never the run.
pub type ReplaySaveFn = Box<dyn Fn(Arc<PlayRecord>, &PlayResult) -> Result<()>>;

/// What the ending screen sends up to its game scene to start replaying the
/// run that just ended, off its "watch replay" button.
pub struct StartReplay;
