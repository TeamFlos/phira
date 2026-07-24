use prpr_auto_offset::NoteEvent;

#[derive(Debug, Clone)]
pub struct StudyRow {
    pub chart_id: i32,
    pub chart_name: String,
    pub notes: usize,
    pub duration_sec: f64,
    pub search_center_sec: f64,
    pub suggested_offset_sec: f64,
    pub lag_sec: f64,
    pub raw_peak: f64,
    pub note_energy: f64,
    pub audio_energy: f64,
    pub normalized_peak: f64,
    pub reliable: bool,
    pub drag_ratio: f64,
    pub chart_listed: Option<bool>,
}

impl StudyRow {
    pub fn header() -> &'static str {
        "chart_id,chart_name,notes,duration_sec,search_center_sec,suggested_offset_sec,lag_sec,raw_peak,note_energy,audio_energy,normalized_peak,reliable,drag_ratio,chart_listed"
    }

    pub fn to_csv(&self) -> String {
        format!(
            "{},{},{},{:.6},{:.6},{:.6},{:.6},{:.9},{:.9},{:.9},{:.9},{},{:.9},{}",
            self.chart_id,
            csv_escape(&self.chart_name),
            self.notes,
            self.duration_sec,
            self.search_center_sec,
            self.suggested_offset_sec,
            self.lag_sec,
            self.raw_peak,
            self.note_energy,
            self.audio_energy,
            self.normalized_peak,
            self.reliable,
            self.drag_ratio,
            csv_optional_bool(self.chart_listed),
        )
    }

    pub fn listing_label(&self) -> &'static str {
        match self.chart_listed {
            Some(true) => "listed",
            Some(false) => "unlisted",
            None => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NoteStats {
    pub events: Vec<NoteEvent>,
    pub drags: usize,
}

impl NoteStats {
    pub fn drag_ratio(&self) -> f64 {
        if self.events.is_empty() {
            0.0
        } else {
            self.drags as f64 / self.events.len() as f64
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FittedPlane {
    pub intercept: f64,
    pub note_coef: f64,
    pub audio_coef: f64,
    pub r2: f64,
    pub rmse: f64,
}

impl FittedPlane {
    pub fn predict_log_raw(&self, note_energy: f64, audio_energy: f64) -> f64 {
        self.intercept + self.note_coef * note_energy.max(1e-12).log10() + self.audio_coef * audio_energy.max(1e-12).log10()
    }
}

fn csv_optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "",
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
