use prpr_auto_offset::{estimate_with, AlignConfig, EnergyDiff, NoteGaussian, Signal, SpectralFlux, SuperFlux};
use std::path::{Path, PathBuf};

const NOTE_SIGMA: f64 = 0.02;

fn config() -> AlignConfig {
    AlignConfig {
        search_range_sec: 0.4,
        sampling_interval_sec: 0.001,
        search_center_sec: 0.0,
    }
}

#[test]
fn analytic_signal_has_expected_sign() {
    let note_times = vec![1.0, 1.75, 2.5, 3.125, 4.0, 5.25, 6.0];
    let true_offset = 0.123;
    let audio = NoteGaussian::new(note_times.iter().map(|t| t + true_offset).collect(), NOTE_SIGMA);
    let note = NoteGaussian::new(note_times, NOTE_SIGMA);
    let result = estimate_with(&audio, &note, 7.0, &config());

    eprintln!("analytic: true={true_offset:.3}s estimated={:.3}s corr={:.4}", result.offset, result.correlation);
    assert!((result.offset - true_offset).abs() <= config().sampling_interval_sec);
}

struct SparseSignal {
    values: Vec<(f64, f32)>,
}

impl Signal for SparseSignal {
    fn samples(&self, ts: &[f64]) -> Vec<f32> {
        ts.iter()
            .map(|&t| {
                self.values
                    .iter()
                    .find(|&&(at, _)| (t - at).abs() < 1e-9)
                    .map(|&(_, value)| value)
                    .unwrap_or(0.0)
            })
            .collect()
    }
}

#[test]
fn normalized_correlation_is_amplitude_scale_invariant() {
    let config = AlignConfig {
        search_range_sec: 0.2,
        sampling_interval_sec: 0.1,
        search_center_sec: 0.0,
    };
    let note = SparseSignal {
        values: vec![(1.0, 1.0), (1.1, 1.0), (1.2, 1.0)],
    };
    let audio = SparseSignal {
        values: vec![(1.0, 1.0), (1.1, 1.0), (1.2, 1.0)],
    };
    let louder_audio = SparseSignal {
        values: vec![(1.0, 10.0), (1.1, 10.0), (1.2, 10.0)],
    };

    let result = estimate_with(&audio, &note, 2.0, &config);
    let louder_result = estimate_with(&louder_audio, &note, 2.0, &config);

    assert_eq!(result.offset, 0.0);
    assert!((result.correlation - 1.0).abs() < 1e-6);
    assert_eq!(louder_result.offset, result.offset);
    assert!((louder_result.correlation - result.correlation).abs() < 1e-6);
    assert!(result.correlation_curve.iter().all(|&(_, v)| (0.0..=1.0).contains(&v)));
}

#[derive(Debug, Clone, Copy)]
enum Frontend {
    Energy,
    Spectral,
    SuperFlux,
}

impl Frontend {
    fn label(self) -> String {
        match self {
            Self::Energy => "energy".to_owned(),
            Self::Spectral => "spectral".to_owned(),
            Self::SuperFlux => "superflux".to_owned(),
        }
    }

    fn estimate(self, pcm: &[f32], sample_rate: u32, duration: f64, note_times: &[f64]) -> BiasMeasurement {
        let note = NoteGaussian::new(note_times.to_vec(), NOTE_SIGMA);
        let result = match self {
            Self::Energy => {
                let audio = EnergyDiff::new(pcm, sample_rate, 10.0, 5.0);
                estimate_with(&audio, &note, duration, &config())
            }
            Self::Spectral => {
                let audio = SpectralFlux::new(pcm, sample_rate, 1024, 512);
                estimate_with(&audio, &note, duration, &config())
            }
            Self::SuperFlux => {
                let audio = SuperFlux::new(pcm, sample_rate, 2048, 1024);
                estimate_with(&audio, &note, duration, &config())
            }
        };
        BiasMeasurement {
            offset: result.offset,
            correlation: result.correlation,
        }
    }
}

struct BiasMeasurement {
    offset: f64,
    correlation: f64,
}

fn run_frontends(
    case_name: &str,
    frontends: &[Frontend],
    pcm: &[f32],
    sample_rate: u32,
    duration: f64,
    note_times: &[f64],
    true_offset: f64,
) -> Vec<(Frontend, BiasMeasurement)> {
    frontends
        .iter()
        .copied()
        .map(|frontend| {
            let measurement = frontend.estimate(pcm, sample_rate, duration, note_times);
            eprintln!(
                "{case_name:>9} {:>8}: true={true_offset:+.3}s estimated={:+.3}s bias={:+.0}ms corr={:.4}",
                frontend.label(),
                measurement.offset,
                (measurement.offset - true_offset) * 1000.0,
                measurement.correlation
            );
            (frontend, measurement)
        })
        .collect()
}

fn assert_bias_within(measurements: &[(Frontend, BiasMeasurement)], true_offset: f64, tolerance: f64) {
    for (frontend, measurement) in measurements {
        assert!(
            (measurement.offset - true_offset).abs() <= tolerance + 1e-9,
            "{} bias exceeded tolerance: true={true_offset:+.3}s estimated={:+.3}s",
            frontend.label(),
            measurement.offset
        );
    }
}

fn assert_frontend_bias_within(measurements: &[(Frontend, BiasMeasurement)], true_offset: f64) {
    for (frontend, measurement) in measurements {
        let tolerance = match frontend {
            Frontend::Energy => 0.005,
            Frontend::Spectral => 0.040,
            Frontend::SuperFlux => 0.005,
        };
        assert!(
            (measurement.offset - true_offset).abs() <= tolerance + 1e-9,
            "{} bias exceeded tolerance: true={true_offset:+.3}s estimated={:+.3}s tolerance={:.0}ms",
            frontend.label(),
            measurement.offset,
            tolerance * 1000.0
        );
    }
}

fn synthetic_click_kernel(sample_rate: u32) -> Vec<f32> {
    let burst_len = (0.030 * sample_rate as f64).round() as usize;
    let attack_len = (0.001 * sample_rate as f64).round().max(1.0) as usize;
    (0..burst_len)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let attack = (i as f32 / attack_len as f32).min(1.0);
            let env = attack * (-t * 150.0).exp();
            let tone = (2.0 * std::f32::consts::PI * 2400.0 * t).sin();
            0.85 * env * tone
        })
        .collect()
}

fn synth_layered_pcm(sample_rate: u32, duration: f64, note_times: &[f64], true_offset: f64, layers: &[&[f32]]) -> Vec<f32> {
    assert!(!layers.is_empty());
    let mut pcm = vec![0.0f32; (duration * sample_rate as f64).round() as usize];
    for (index, &note_time) in note_times.iter().enumerate() {
        let onset = note_time + true_offset;
        assert!(onset >= 0.0, "synthetic onset must be non-negative");
        let start = (onset * sample_rate as f64).round() as usize;
        let layer = layers[index % layers.len()];
        assert!(start + layer.len() < pcm.len(), "synthetic layer must fit in the buffer");
        for (i, &sample) in layer.iter().enumerate() {
            pcm[start + i] += sample;
        }
    }
    pcm
}

fn assert_layers_are_sample_accurate(pcm: &[f32], sample_rate: u32, note_times: &[f64], true_offset: f64, layers: &[&[f32]]) {
    for (index, &note_time) in note_times.iter().enumerate() {
        let start = ((note_time + true_offset) * sample_rate as f64).round() as usize;
        assert_eq!(pcm[start.saturating_sub(1)], 0.0, "sample before onset must be silent");
        let layer = layers[index % layers.len()];
        for i in 0..layer.len().min(256) {
            assert_eq!(pcm[start + i], layer[i], "layer must be placed exactly at the requested onset sample");
        }
    }
}

fn decode_mono_pcm(path: &Path) -> (Vec<f32>, u32) {
    let clip = prpr_avc::demux_audio(path.to_str().unwrap()).unwrap().unwrap();
    let sample_rate = clip.sample_rate();
    let pcm = clip.frames().iter().map(|frame| (frame.0 + frame.1) / 2.0).collect();
    (pcm, sample_rate)
}

fn asset_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets").join(name)
}

#[test]
fn measure_audio_frontend_bias_on_synthetic_clicks() {
    let sample_rate = 44_100u32;
    let duration = 9.0;
    let note_times = vec![1.0, 1.75, 2.5, 3.125, 4.0, 5.25, 6.0, 7.1];
    let click = synthetic_click_kernel(sample_rate);
    assert_eq!(click[0], 0.0, "the synthetic half-sine starts exactly at the onset sample");
    assert!(click[1].abs() > 0.0, "the first post-onset sample must contain the synthetic click");

    let frontends = [Frontend::Energy, Frontend::Spectral, Frontend::SuperFlux];
    for true_offset in [-0.157, 0.0, 0.123, 0.278] {
        let pcm = synth_layered_pcm(sample_rate, duration, &note_times, true_offset, &[&click]);
        assert_layers_are_sample_accurate(&pcm, sample_rate, &note_times, true_offset, &[&click]);

        let measurements = run_frontends("synthetic", &frontends, &pcm, sample_rate, duration, &note_times, true_offset);
        assert_bias_within(&measurements, true_offset, 0.005);
    }
}

#[test]
fn measure_audio_frontend_bias_on_synthesized_hit_sounds() {
    let sounds: Vec<(&str, Vec<f32>, u32)> = ["click.ogg", "flick.ogg", "drag.ogg"]
        .into_iter()
        .map(|name| {
            let (pcm, sample_rate) = decode_mono_pcm(&asset_path(name));
            (name, pcm, sample_rate)
        })
        .collect();
    let sample_rate = sounds[0].2;
    for (name, _, rate) in &sounds {
        assert_eq!(*rate, sample_rate, "{name} must use the same sample rate as the other hit sounds");
    }

    let mut cases: Vec<(&str, Vec<&[f32]>)> = sounds.iter().map(|(name, pcm, _)| (*name, vec![pcm.as_slice()])).collect();
    cases.push(("mixed", sounds.iter().map(|(_, pcm, _)| pcm.as_slice()).collect()));

    let frontends = [Frontend::Energy, Frontend::Spectral, Frontend::SuperFlux];
    for (name, layers) in cases {
        let max_layer_len = layers.iter().map(|layer| layer.len()).max().unwrap();
        let spacing = max_layer_len as f64 / sample_rate as f64 + 0.5;
        let note_times: Vec<f64> = (0..8).map(|i| 1.0 + i as f64 * spacing).collect();
        let duration = note_times.last().copied().unwrap() + spacing + 1.0;
        for true_offset in [-0.123, 0.0, 0.157] {
            let pcm = synth_layered_pcm(sample_rate, duration, &note_times, true_offset, &layers);
            assert_layers_are_sample_accurate(&pcm, sample_rate, &note_times, true_offset, &layers);
            let measurements = run_frontends(name, &frontends, &pcm, sample_rate, duration, &note_times, true_offset);
            assert_frontend_bias_within(&measurements, true_offset);
        }
    }
}
