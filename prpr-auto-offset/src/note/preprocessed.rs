use crate::Signal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoOffsetNoteKind {
    Tap,
    Hold,
    Flick,
    Slide,
}

#[derive(Clone, Copy, Debug)]
pub struct NoteEvent {
    pub time: f64,
    pub kind: AutoOffsetNoteKind,
}

impl NoteEvent {
    pub fn new(time: f64, kind: AutoOffsetNoteKind) -> Self {
        Self { time, kind }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NotePreprocessConfig {
    pub max_notes_per_time: f32,
    pub slide_run_weight: f32,
    pub min_slide_run_len: usize,
    pub max_slide_interval_sec: f64,
    pub equal_interval_tolerance_sec: f64,
    pub time_epsilon_sec: f64,
    pub slide_weight: f32,
}

impl Default for NotePreprocessConfig {
    fn default() -> Self {
        Self {
            max_notes_per_time: 2.0,
            slide_run_weight: 0.2,
            min_slide_run_len: 5,
            max_slide_interval_sec: 0.12,
            equal_interval_tolerance_sec: 0.008,
            time_epsilon_sec: 1e-4,
            slide_weight: 0.8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TimeGroup {
    time: f64,
    slides: usize,
    others: usize,
}

#[derive(Clone, Copy, Debug)]
struct WeightedNote {
    time: f64,
    weight: f32,
}

/// A Gaussian note signal with chart-note preprocessing for auto-offset.
///
/// The preprocessing caps simultaneous notes and downweights dense, evenly
/// spaced slide runs that often behave more like visual chart texture than
/// audio onsets.
pub struct PreprocessedNoteGaussian {
    notes: Vec<WeightedNote>,
    sigma: f64,
}

impl PreprocessedNoteGaussian {
    pub fn new(notes: Vec<NoteEvent>, sigma: f64) -> Self {
        Self::with_config(notes, sigma, NotePreprocessConfig::default())
    }

    pub fn with_config(notes: Vec<NoteEvent>, sigma: f64, config: NotePreprocessConfig) -> Self {
        Self {
            notes: preprocess_notes(notes, config),
            sigma,
        }
    }
}

impl Signal for PreprocessedNoteGaussian {
    fn samples(&self, ts: &[f64]) -> Vec<f32> {
        if ts.is_empty() || self.notes.is_empty() {
            return vec![0.0; ts.len()];
        }
        let inv_sigma = 1.0 / self.sigma;
        ts.iter()
            .map(|&t| {
                self.notes
                    .iter()
                    .map(|note| {
                        let d = (t - note.time) * inv_sigma;
                        note.weight * ((-0.5 * d * d).exp()) as f32
                    })
                    .sum::<f32>()
            })
            .collect()
    }
}

fn preprocess_notes(mut notes: Vec<NoteEvent>, config: NotePreprocessConfig) -> Vec<WeightedNote> {
    notes.retain(|note| note.time.is_finite() && note.time >= 0.0);
    notes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());

    let groups = group_notes(&notes, config.time_epsilon_sec.max(0.0));
    let slide_run_groups = detect_slide_runs(&groups, config);
    let mut output = Vec::with_capacity(groups.len());

    for (index, group) in groups.iter().enumerate() {
        let slide_weight = if slide_run_groups[index] {
            config.slide_run_weight.max(0.0)
        } else {
            config.slide_weight
        };
        let weight = group.others as f32 + group.slides as f32 * slide_weight;
        let weight = weight.min(config.max_notes_per_time.max(0.0));
        if weight > 0.0 {
            output.push(WeightedNote { time: group.time, weight });
        }
    }

    output
}

fn group_notes(notes: &[NoteEvent], time_epsilon: f64) -> Vec<TimeGroup> {
    let mut groups: Vec<TimeGroup> = Vec::new();
    for note in notes {
        if let Some(last) = groups.last_mut() {
            if (note.time - last.time).abs() <= time_epsilon {
                add_note_to_group(last, *note);
                continue;
            }
        }
        let mut group = TimeGroup {
            time: note.time,
            slides: 0,
            others: 0,
        };
        add_note_to_group(&mut group, *note);
        groups.push(group);
    }
    groups
}

fn add_note_to_group(group: &mut TimeGroup, note: NoteEvent) {
    if note.kind == AutoOffsetNoteKind::Slide {
        group.slides += 1;
    } else {
        group.others += 1;
    }
}

fn detect_slide_runs(groups: &[TimeGroup], config: NotePreprocessConfig) -> Vec<bool> {
    let mut marked = vec![false; groups.len()];
    let mut segment = Vec::new();

    for (index, group) in groups.iter().enumerate() {
        if group.slides > 0 && group.others == 0 {
            segment.push(index);
        } else {
            mark_even_slide_segments(groups, &segment, config, &mut marked);
            segment.clear();
        }
    }
    mark_even_slide_segments(groups, &segment, config, &mut marked);

    marked
}

fn mark_even_slide_segments(groups: &[TimeGroup], segment: &[usize], config: NotePreprocessConfig, marked: &mut [bool]) {
    if segment.len() < config.min_slide_run_len {
        return;
    }

    let mut start = 0;
    let mut base_interval = None;
    for pos in 1..segment.len() {
        let interval = groups[segment[pos]].time - groups[segment[pos - 1]].time;
        let valid_interval = interval > 0.0 && interval <= config.max_slide_interval_sec;
        let same_interval = base_interval.is_none_or(|base: f64| (interval - base).abs() <= config.equal_interval_tolerance_sec);
        if !valid_interval {
            mark_run(segment, start, pos - 1, config.min_slide_run_len, marked);
            start = pos;
            base_interval = None;
        } else if same_interval {
            base_interval.get_or_insert(interval);
        } else {
            mark_run(segment, start, pos - 1, config.min_slide_run_len, marked);
            start = pos - 1;
            base_interval = Some(interval);
        }
    }
    mark_run(segment, start, segment.len() - 1, config.min_slide_run_len, marked);
}

fn mark_run(segment: &[usize], start: usize, end: usize, min_len: usize, marked: &mut [bool]) {
    if end + 1 - start < min_len {
        return;
    }
    for &group_index in &segment[start..=end] {
        marked[group_index] = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_simultaneous_notes() {
        let notes = vec![
            NoteEvent::new(1.0, AutoOffsetNoteKind::Tap),
            NoteEvent::new(1.0, AutoOffsetNoteKind::Tap),
            NoteEvent::new(1.0, AutoOffsetNoteKind::Flick),
            NoteEvent::new(1.0, AutoOffsetNoteKind::Slide),
        ];
        let signal = PreprocessedNoteGaussian::new(notes, 0.001);
        assert!((signal.samples(&[1.0])[0] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn downweights_dense_even_slide_runs() {
        let notes = (0..5).map(|i| NoteEvent::new(i as f64 * 0.05, AutoOffsetNoteKind::Slide)).collect();
        let signal = PreprocessedNoteGaussian::new(notes, 0.001);
        assert!((signal.samples(&[0.10])[0] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn other_note_breaks_slide_run() {
        let notes = vec![
            NoteEvent::new(0.00, AutoOffsetNoteKind::Slide),
            NoteEvent::new(0.05, AutoOffsetNoteKind::Slide),
            NoteEvent::new(0.10, AutoOffsetNoteKind::Flick),
            NoteEvent::new(0.15, AutoOffsetNoteKind::Slide),
            NoteEvent::new(0.20, AutoOffsetNoteKind::Slide),
            NoteEvent::new(0.25, AutoOffsetNoteKind::Slide),
        ];
        let signal = PreprocessedNoteGaussian::new(notes, 0.001);
        assert!((signal.samples(&[0.00])[0] - 0.8).abs() < 1e-6);
    }
}
