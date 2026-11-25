//! Time between consecutive host presents. Every completed interval belongs to
//! one reporting window, including an interval that crosses the window boundary.

use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(5);
// End a window early at very high present rates to bound storage and sorting.
const MAX_INTERVALS: usize = 4096;

#[derive(Default)]
pub(crate) struct FrameTimes {
    previous: Option<Instant>,
    elapsed: Duration,
    intervals: Vec<Duration>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Summary {
    pub(crate) intervals: usize,
    pub(crate) mean_ms: f64,
    pub(crate) median: Duration,
    pub(crate) p95: Duration,
    pub(crate) longest: Duration,
    pub(crate) over_100ms: usize,
    pub(crate) over_1s: usize,
}

impl FrameTimes {
    /// DXGI stopped presenting (for example, a minimized window). Keep
    /// completed intervals, and start timing again from the next success.
    pub(crate) fn interrupted(&mut self) {
        self.previous = None;
    }

    /// How long the present interval ending at `now` took, without ending the
    /// reporting window. Zero for the first present after an interruption.
    #[cfg(any(test, feature = "diagnostics"))]
    pub(crate) fn previous_interval(&self, now: Instant) -> Duration {
        self.previous.map_or(Duration::ZERO, |p| now - p)
    }

    pub(crate) fn present(&mut self, now: Instant) -> Option<Summary> {
        let interval = now - self.previous.replace(now)?;
        self.intervals.push(interval);
        self.elapsed += interval;
        let n = self.intervals.len();
        if self.elapsed < WINDOW && n < MAX_INTERVALS {
            return None;
        }
        self.intervals.sort_unstable();
        let intervals = &self.intervals;
        let lo = intervals[(n - 1) / 2];
        let hi = intervals[n / 2];
        let summary = Summary {
            intervals: n,
            mean_ms: self.elapsed.as_secs_f64() * 1000.0 / n as f64,
            median: lo + (hi - lo) / 2,
            p95: intervals[(95 * n).div_ceil(100) - 1],
            longest: intervals[n - 1],
            over_100ms: n - intervals.partition_point(|&d| d <= Duration::from_millis(100)),
            over_1s: n - intervals.partition_point(|&d| d <= Duration::from_secs(1)),
        };
        self.intervals.clear();
        self.elapsed = Duration::ZERO;
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalls_and_percentiles_cover_each_present_interval_once() {
        let mut times = FrameTimes::default();
        let ms = Duration::from_millis;
        let mut now = Instant::now();
        assert_eq!(times.present(now), None);

        // Twenty intervals: sixteen at 16 ms, then 100, 101, 1000 and
        // 4000 ms. A long first launch before the first present is excluded.
        for dt in [16; 16].into_iter().chain([100, 101, 1000]) {
            now += ms(dt);
            assert_eq!(times.present(now), None);
        }
        now += ms(4000);
        let first = times.present(now).unwrap();
        assert_eq!(
            first,
            Summary {
                intervals: 20,
                mean_ms: 5457.0 / 20.0,
                median: ms(16),
                p95: ms(1000),
                longest: ms(4000),
                over_100ms: 3,
                over_1s: 1,
            }
        );
        now += ms(5010);
        let next = times.present(now).unwrap();
        assert_eq!(
            (next.intervals, next.median, next.p95, next.longest),
            (1, ms(5010), ms(5010), ms(5010))
        );
        assert_eq!((next.over_100ms, next.over_1s), (1, 1));
    }

    #[test]
    fn resuming_presentation_excludes_the_occluded_time() {
        let mut times = FrameTimes::default();
        let start = Instant::now();
        assert_eq!(times.present(start), None);
        assert_eq!(times.present(start + Duration::from_secs(2)), None);
        times.interrupted();
        assert_eq!(times.present(start + Duration::from_secs(62)), None);
        let s = times.present(start + Duration::from_secs(65)).unwrap();
        assert_eq!(
            (s.intervals, s.mean_ms, s.longest),
            (2, 2500.0, Duration::from_secs(3))
        );
    }

    #[test]
    fn a_full_fast_window_is_bounded_and_keeps_its_even_median() {
        let mut times = FrameTimes::default();
        let mut now = Instant::now();
        assert_eq!(times.present(now), None);
        for i in 1..=MAX_INTERVALS {
            now += Duration::from_micros(if i % 2 == 0 { 300 } else { 100 });
            let result = times.present(now);
            if i == MAX_INTERVALS {
                let s = result.unwrap();
                assert_eq!(s.intervals, MAX_INTERVALS);
                assert_eq!(s.median, Duration::from_micros(200));
                assert_eq!(s.p95, Duration::from_micros(300));
                assert_eq!((s.over_100ms, s.over_1s), (0, 0));
            } else {
                assert_eq!(result, None);
            }
        }
    }

    #[test]
    fn previous_interval_reports_the_open_interval_without_ending_it() {
        let mut times = FrameTimes::default();
        let ms = Duration::from_millis;
        let mut now = Instant::now();
        assert_eq!(times.previous_interval(now), ms(0));
        assert_eq!(times.present(now), None);
        assert_eq!(times.previous_interval(now), ms(0));
        now += ms(37);
        assert_eq!(times.previous_interval(now), ms(37));
        // Reporting the interval does not consume it or end the window.
        assert_eq!(times.previous_interval(now), ms(37));
        assert_eq!(times.present(now).map(|s| s.intervals), None);
        now += ms(5010);
        assert_eq!(times.previous_interval(now), ms(5010));
        let s = times.present(now).unwrap();
        assert_eq!(s.intervals, 2);
        assert_eq!(s.longest, ms(5010));
    }
}
