//! Hold geometry with independent official-chart body speed.
/// `body_speed` is in normalized chart units/second, after projection/scaling.
/// Native RPE holds keep their integrated tail when the override is absent.
pub(crate) fn tail_height(head: f64, start: f64, end: f64, now: f64, body_speed: Option<f64>, integrated_tail: f64) -> f64 {
    body_speed.map_or(integrated_tail, |speed| head + (end - now.max(start)).max(0.) * speed)
}

#[cfg(test)]
mod tests {
    use super::tail_height;
    #[test]
    fn official_length_does_not_follow_line_speed_changes() {
        for integral in [-100., 0., 1000.] {
            assert_eq!(tail_height(10., 2., 4., 1., Some(5.), integral), 20.);
        }
    }
    #[test]
    fn held_body_shrinks_by_remaining_time() {
        assert_eq!(tail_height(0., 2., 4., 2., Some(5.), 100.), 10.);
        assert_eq!(tail_height(0., 2., 4., 3., Some(5.), 100.), 5.);
        assert_eq!(tail_height(0., 2., 4., 5., Some(5.), 100.), 0.);
    }
    #[test]
    fn native_rpe_integral_tail_is_unchanged() {
        assert_eq!(tail_height(10., 2., 4., 1., None, 37.), 37.);
        assert_eq!(tail_height(0., 2., 4., 3., None, -2.), -2.);
    }
    #[test]
    fn constant_speed_matches_the_existing_integral() {
        assert_eq!(tail_height(10., 2., 4., 1., Some(5.), 20.), 20.);
        assert_eq!(tail_height(0., 2., 4., 3., Some(5.), 5.), 5.);
    }
}
