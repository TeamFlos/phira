//! Statistics derived from a [`PlayRecord`].
//!
//! Nothing here is stored or uploaded — every value is recomputed from a
//! record, so a stat can never go stale or be silently left at its default
//! because some `finish()` call was forgotten. Accuracies are exposed as
//! methods returning [`Option`] rather than as fields, so that a group with no
//! notes in it reads as "no data" instead of as a flawless 100%.

use crate::{
    judge::Judgement,
    play_record::{NoteKindTag, NoteRecord, PlayRecord},
};
use serde::{Deserialize, Serialize};

/// Weight of a Good in the accuracy formula, mirroring the judge's scoring.
pub const GOOD_WEIGHT: f32 = 0.65;

pub const HIT_ERROR_BIN_WIDTH_MS: i16 = 5;
pub const HIT_ERROR_RANGE_MS: i16 = 220;
/// Bin centers run from `-220ms` through `+220ms` inclusive, in 5ms steps.
pub const HIT_ERROR_BIN_COUNT: usize = (HIT_ERROR_RANGE_MS as usize * 2 / HIT_ERROR_BIN_WIDTH_MS as usize) + 1;

/// Number of equal slices the chart is cut into for the progress breakdown.
pub const SEGMENT_COUNT: usize = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgementCounts {
    /// Indexed by `Judgement as usize`: Perfect, Good, Bad, Miss.
    pub counts: [u32; 4],
}

impl JudgementCounts {
    pub fn push(&mut self, judgement: Judgement) {
        self.counts[judgement as usize] += 1;
    }

    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    pub fn perfect(&self) -> u32 {
        self.counts[Judgement::Perfect as usize]
    }

    pub fn good(&self) -> u32 {
        self.counts[Judgement::Good as usize]
    }

    pub fn bad(&self) -> u32 {
        self.counts[Judgement::Bad as usize]
    }

    pub fn miss(&self) -> u32 {
        self.counts[Judgement::Miss as usize]
    }

    /// `None` when the group contains no notes at all — a chart with no flicks,
    /// or a segment that falls in a gap. Rendering those as 100% would be a lie.
    pub fn accuracy(&self) -> Option<f32> {
        let total = self.total();
        (total != 0).then(|| (self.perfect() as f32 + self.good() as f32 * GOOD_WEIGHT) / total as f32)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EarlyLate {
    pub early: u32,
    pub late: u32,
}

impl EarlyLate {
    fn push(&mut self, diff: f32) {
        if diff < 0. {
            self.early += 1;
        } else if diff > 0. {
            self.late += 1;
        }
    }

    pub fn total(&self) -> u32 {
        self.early + self.late
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitError {
    /// Counts per 5ms bin from `-220ms` to `+220ms`; everything with a timing
    /// error contributes, including Bads, which are clamped into the end bins.
    pub histogram: Vec<u32>,
    /// Mean and standard deviation over Perfect and Good only. Bads sit at the
    /// very edge of the judgement window and would otherwise dominate both.
    pub mean_ms: f32,
    pub std_dev_ms: f32,
    /// How many notes went into `mean_ms` / `std_dev_ms`.
    pub sample_count: u32,
    /// Early/late split per judgement, indexed by `Judgement as usize`. Unlike
    /// the result screen this covers Perfects too, which is where the useful
    /// offset signal lives.
    pub early_late: [EarlyLate; 4],
}

impl Default for HitError {
    fn default() -> Self {
        Self {
            histogram: vec![0; HIT_ERROR_BIN_COUNT],
            mean_ms: 0.,
            std_dev_ms: 0.,
            sample_count: 0,
            early_late: Default::default(),
        }
    }
}

impl HitError {
    /// Index of the bin a timing error in milliseconds falls into.
    pub fn bin_of(error_ms: f32) -> usize {
        let range = HIT_ERROR_RANGE_MS as f32;
        ((error_ms.clamp(-range, range) + range) / HIT_ERROR_BIN_WIDTH_MS as f32).round() as usize
    }

    /// Center of a bin in milliseconds.
    pub fn bin_center_ms(bin: usize) -> f32 {
        bin as f32 * HIT_ERROR_BIN_WIDTH_MS as f32 - HIT_ERROR_RANGE_MS as f32
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayStats {
    pub total: JudgementCounts,
    pub hit_error: HitError,
    /// Indexed by `NoteKindTag as usize`.
    pub by_kind: [JudgementCounts; NoteKindTag::COUNT],
    /// Indexed by judge line id. Lines with no notes stay empty.
    pub by_line: Vec<JudgementCounts>,
    /// The chart cut into [`SEGMENT_COUNT`] equal slices of time.
    pub segments: [JudgementCounts; SEGMENT_COUNT],
    /// Notes hit alone versus notes sharing their time with another note.
    /// Multi-hits are usually where accuracy quietly falls apart.
    pub single: JudgementCounts,
    pub multiple: JudgementCounts,
    /// Holds whose head was hit but which were released early. These are
    /// counted as Misses everywhere else, indistinguishable from holds that
    /// were never touched at all.
    pub hold_broken: u32,
}

impl PlayStats {
    pub fn new(record: &PlayRecord) -> Self {
        let mut stats = Self {
            by_line: vec![JudgementCounts::default(); record.notes.iter().map(|it| it.line as usize + 1).max().unwrap_or(0)],
            ..Default::default()
        };

        // Slice by the last note rather than by the track length: charts often
        // end well before their music does, and empty trailing segments would
        // otherwise show up as gaps.
        let span = record.notes.iter().fold(0_f32, |acc, note| acc.max(note.time));

        let mut errors = Vec::new();
        for note in &record.notes {
            stats.total.push(note.judgement);
            stats.by_kind[note.kind as usize].push(note.judgement);
            if let Some(line) = stats.by_line.get_mut(note.line as usize) {
                line.push(note.judgement);
            }
            if note.is_multiple() { &mut stats.multiple } else { &mut stats.single }.push(note.judgement);
            if note.is_hold_broken() {
                stats.hold_broken += 1;
            }

            let progress = if span > 0. { note.time / span } else { 0. };
            let segment = ((progress * SEGMENT_COUNT as f32) as usize).min(SEGMENT_COUNT - 1);
            stats.segments[segment].push(note.judgement);

            let Some(diff) = note.diff else { continue };
            let error_ms = diff * 1000.;
            stats.hit_error.early_late[note.judgement as usize].push(error_ms);
            stats.hit_error.histogram[HitError::bin_of(error_ms)] += 1;
            if matches!(note.judgement, Judgement::Perfect | Judgement::Good) {
                errors.push(error_ms);
            }
        }

        if !errors.is_empty() {
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            let variance = errors.iter().map(|it| (it - mean).powi(2)).sum::<f32>() / errors.len() as f32;
            stats.hit_error.mean_ms = mean;
            stats.hit_error.std_dev_ms = variance.sqrt();
            stats.hit_error.sample_count = errors.len() as u32;
        }

        stats
    }

    /// Overall accuracy, matching the formula the result screen uses.
    pub fn accuracy(&self) -> Option<f32> {
        self.total.accuracy()
    }
}

impl NoteRecord {
    pub fn is_multiple(&self) -> bool {
        self.flags.contains(crate::play_record::NoteFlags::MULTIPLE)
    }

    pub fn is_hold_broken(&self) -> bool {
        self.flags.contains(crate::play_record::NoteFlags::HOLD_BROKEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play_record::{NoteFlags, PlayMeta, PLAY_RECORD_VERSION};

    fn note(time: f32, kind: NoteKindTag, judgement: Judgement, diff: Option<f32>) -> NoteRecord {
        NoteRecord {
            line: 0,
            time,
            kind,
            judgement,
            diff,
            flags: NoteFlags::empty(),
        }
    }

    fn record(notes: Vec<NoteRecord>) -> PlayRecord {
        PlayRecord {
            version: PLAY_RECORD_VERSION,
            meta: PlayMeta::default(),
            notes,
            touch_frames: Vec::new(),
        }
    }

    #[test]
    fn histogram_bins_are_inclusive_on_both_ends() {
        assert_eq!(HIT_ERROR_BIN_COUNT, 89);
        assert_eq!(HitError::bin_of(-220.), 0);
        assert_eq!(HitError::bin_of(0.), 44);
        assert_eq!(HitError::bin_of(220.), HIT_ERROR_BIN_COUNT - 1);
        // Out-of-range values clamp rather than panic.
        assert_eq!(HitError::bin_of(-1000.), 0);
        assert_eq!(HitError::bin_of(1000.), HIT_ERROR_BIN_COUNT - 1);
        assert_eq!(HitError::bin_center_ms(44), 0.);
    }

    #[test]
    fn accuracy_uses_the_game_weights() {
        let mut counts = JudgementCounts::default();
        for judgement in [Judgement::Perfect, Judgement::Good, Judgement::Bad, Judgement::Miss] {
            counts.push(judgement);
        }
        assert!((counts.accuracy().unwrap() - 0.4125).abs() < f32::EPSILON);
    }

    #[test]
    fn an_empty_group_has_no_accuracy_rather_than_a_perfect_one() {
        assert_eq!(JudgementCounts::default().accuracy(), None);

        // A chart with no flicks must not report flawless flick accuracy.
        let stats = PlayStats::new(&record(vec![note(0., NoteKindTag::Click, Judgement::Good, Some(0.01))]));
        assert_eq!(stats.by_kind[NoteKindTag::Flick as usize].accuracy(), None);
        assert_eq!(stats.by_kind[NoteKindTag::Click as usize].accuracy(), Some(GOOD_WEIGHT));
        assert_eq!(stats.multiple.accuracy(), None);
    }

    #[test]
    fn segments_span_the_notes_not_the_track() {
        // Every note sits in the first tenth of a long track, but the last note
        // must still land in the last segment.
        let mut rec = record(
            (0..10)
                .map(|i| note(i as f32, NoteKindTag::Click, Judgement::Perfect, Some(0.)))
                .collect(),
        );
        rec.meta.track_length = 600.;
        let stats = PlayStats::new(&rec);
        assert!(stats.segments.iter().all(|it| it.total() == 1), "{:?}", stats.segments);
    }

    #[test]
    fn bads_reach_the_histogram_but_not_the_deviation() {
        let stats = PlayStats::new(&record(vec![
            note(0., NoteKindTag::Click, Judgement::Perfect, Some(0.01)),
            note(1., NoteKindTag::Click, Judgement::Perfect, Some(-0.01)),
            note(2., NoteKindTag::Click, Judgement::Bad, Some(0.21)),
        ]));
        assert_eq!(stats.hit_error.sample_count, 2);
        assert_eq!(stats.hit_error.mean_ms, 0.);
        assert_eq!(stats.hit_error.std_dev_ms, 10.);
        assert_eq!(stats.hit_error.histogram.iter().sum::<u32>(), 3);
        assert_eq!(stats.hit_error.histogram[HitError::bin_of(210.)], 1);
    }

    #[test]
    fn early_late_covers_perfects_too() {
        let stats = PlayStats::new(&record(vec![
            note(0., NoteKindTag::Click, Judgement::Perfect, Some(-0.01)),
            note(1., NoteKindTag::Click, Judgement::Perfect, Some(-0.02)),
            note(2., NoteKindTag::Click, Judgement::Good, Some(0.1)),
        ]));
        assert_eq!(stats.hit_error.early_late[Judgement::Perfect as usize], EarlyLate { early: 2, late: 0 });
        assert_eq!(stats.hit_error.early_late[Judgement::Good as usize], EarlyLate { early: 0, late: 1 });
    }

    #[test]
    fn broken_holds_and_multi_hits_are_split_out() {
        let mut broken = note(0., NoteKindTag::Hold, Judgement::Miss, None);
        broken.flags = NoteFlags::HOLD_BROKEN | NoteFlags::MULTIPLE;
        let mut untouched = note(1., NoteKindTag::Hold, Judgement::Miss, None);
        untouched.flags = NoteFlags::MULTIPLE;
        let stats = PlayStats::new(&record(vec![broken, untouched, note(2., NoteKindTag::Click, Judgement::Perfect, Some(0.))]));

        assert_eq!(stats.hold_broken, 1);
        assert_eq!(stats.by_kind[NoteKindTag::Hold as usize].miss(), 2);
        assert_eq!(stats.multiple.total(), 2);
        assert_eq!(stats.multiple.accuracy(), Some(0.));
        assert_eq!(stats.single.accuracy(), Some(1.));
    }
}
