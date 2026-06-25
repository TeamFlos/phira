use prpr_auto_offset::{estimate_with, AlignConfig, EnergyDiff, NoteGaussian, SpectralFlux, SuperFlux};

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
    let audio = NoteGaussian::new(note_times.iter().map(|t| t + true_offset).collect(), 0.02);
    let note = NoteGaussian::new(note_times, 0.02);
    let result = estimate_with(&audio, &note, 7.0, &config());

    eprintln!("analytic: true={true_offset:.3}s estimated={:.3}s corr={:.4}", result.offset, result.correlation);
    assert!((result.offset - true_offset).abs() <= config().sampling_interval_sec);
}

#[test]
fn measure_audio_frontend_bias_on_synthetic_clicks() {
    let sample_rate = 44_100u32;
    let duration = 9.0;
    let note_times = vec![1.0, 1.75, 2.5, 3.125, 4.0, 5.25, 6.0, 7.1];
    for true_offset in [-0.157, 0.0, 0.123, 0.278] {
        let mut pcm = vec![0.0f32; (duration * sample_rate as f64) as usize];

        for &note_time in &note_times {
            let onset = note_time + true_offset;
            let start = (onset * sample_rate as f64).round() as usize;
            let burst_len = (0.035 * sample_rate as f64).round() as usize;
            for i in 0..burst_len {
                let idx = start + i;
                if idx >= pcm.len() {
                    break;
                }
                let t = i as f32 / sample_rate as f32;
                let env = (-t * 120.0).exp();
                let tone = (2.0 * std::f32::consts::PI * 2200.0 * t).sin();
                pcm[idx] += 0.8 * env * tone;
            }
        }

        let note = NoteGaussian::new(note_times.clone(), 0.02);

        let energy = EnergyDiff::new(&pcm, sample_rate, 10.0, 5.0);
        let result = estimate_with(&energy, &note, duration, &config());
        eprintln!(
            "energy:   true={true_offset:+.3}s estimated={:+.3}s bias={:+.0}ms corr={:.4}",
            result.offset,
            (result.offset - true_offset) * 1000.0,
            result.correlation
        );
        assert!((result.offset - true_offset).abs() <= 0.005);

        let spectral = SpectralFlux::new(&pcm, sample_rate, 1024, 512);
        let result = estimate_with(&spectral, &note, duration, &config());
        eprintln!(
            "spectral: true={true_offset:+.3}s estimated={:+.3}s bias={:+.0}ms corr={:.4}",
            result.offset,
            (result.offset - true_offset) * 1000.0,
            result.correlation
        );
        assert!((result.offset - true_offset).abs() <= 0.005);

        let superflux = SuperFlux::new(&pcm, sample_rate, 2048, 1024);
        let result = estimate_with(&superflux, &note, duration, &config());
        eprintln!(
            "super:    true={true_offset:+.3}s estimated={:+.3}s bias={:+.0}ms corr={:.4}",
            result.offset,
            (result.offset - true_offset) * 1000.0,
            result.correlation
        );
        assert!((result.offset - true_offset).abs() <= 0.005);
    }
}
