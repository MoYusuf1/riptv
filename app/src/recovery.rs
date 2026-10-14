//! Live-only recovery policy. Constant time and memory; no requests, timers, strings or allocation.
#[derive(Clone, Copy, Default)]
pub(crate) struct Recovery {
    attempts: u8,
    serial: u32,
    pending: Option<u64>,
    healthy_since: Option<u64>,
    stopped: bool,
    manual: bool,
    force: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    Wait,
    Retry { serial: u32, attempt: u8 },
    GiveUp,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Transport {
    pub last_data: u64,
    pub waiting_since: u64,
    pub received: u64,
    pub reading: bool,
    pub finished: bool,
    pub terminal: bool,
    pub retry_after: u64,
}

pub(crate) struct Sample {
    pub now: u64,
    pub position: f64,
    pub frames: u64,
    pub has_video: bool,
    pub visible: bool,
    pub ahead: f64,
    pub paused: bool,
    pub holding: bool,
    pub transport: Transport,
}

pub(crate) struct Health {
    pub healthy: bool,
    pub stalled: bool,
    pub stuck: bool,
}
pub(crate) struct Monitor {
    position: f64,
    frames: u64,
    advanced_at: u64,
    frame_at: u64,
}
impl Monitor {
    pub(crate) fn new(now: u64) -> Self {
        Self {
            position: 0.0,
            frames: 0,
            advanced_at: now,
            frame_at: now,
        }
    }
    pub(crate) fn observe(&mut self, s: Sample) -> Health {
        let advanced = (s.position - self.position).abs() >= 0.05;
        if advanced || s.paused || s.holding || !s.visible {
            self.advanced_at = s.now;
        }
        if s.frames != self.frames || s.paused || s.holding || !s.visible {
            self.frame_at = s.now;
        }
        self.position = s.position;
        self.frames = s.frames;
        let p = s.transport;
        let stalled = p.reading
            && s.now.saturating_sub(p.last_data.max(p.waiting_since))
                >= if p.received == 0 { 20_000 } else { 8000 };
        let stuck = s.visible
            && !s.paused
            && !s.holding
            && (s.now.saturating_sub(p.last_data) < 2000 || !p.reading)
            && s.ahead >= 3.0
            && (s.now.saturating_sub(self.advanced_at) >= 8000
                || s.has_video && s.frames > 0 && s.now.saturating_sub(self.frame_at) >= 8000);
        Health {
            healthy: !s.paused
                && !s.holding
                && advanced
                && !p.finished
                && !stalled
                && !stuck
                && s.ahead >= 2.0,
            stalled,
            stuck,
        }
    }
}

impl Recovery {
    pub(crate) fn reserve_for(&self, smoother: bool) -> f64 {
        if smoother {
            (self.reserve() + 6.0).min(24.0)
        } else {
            self.reserve()
        }
    }
    pub(crate) fn reserve(&self) -> f64 {
        (6 + u32::from(self.attempts) * 3).min(18) as f64
    }
    pub(crate) fn request(&mut self, now: u64, jitter: u64, minimum_delay: u64) -> bool {
        if self.stopped || self.pending.is_some() {
            return false;
        }
        let delay = if self.attempts >= 5 {
            0
        } else {
            [500, 1000, 2000, 4000, 8000][self.attempts as usize] + jitter.min(250)
        };
        self.pending = Some(now.saturating_add(delay.max(minimum_delay)));
        self.healthy_since = None;
        true
    }
    pub(crate) fn healthy(&mut self, now: u64, good: bool) {
        if self.manual {
            return;
        }
        if !good {
            self.healthy_since = None;
            return;
        }
        self.pending = None;
        self.force = false;
        let since = *self.healthy_since.get_or_insert(now);
        if now.saturating_sub(since) >= 60_000 {
            self.attempts = 0;
        }
    }
    pub(crate) fn poll(&mut self, now: u64, paused: bool, ahead: f64) -> Step {
        if paused {
            self.pending = None;
            self.manual = false;
            self.force = false;
            self.healthy_since = None;
            return Step::Wait;
        }
        let Some(due) = self.pending else {
            return Step::Wait;
        };
        if now < due || ahead > 1.5 && !self.manual && !self.force {
            return Step::Wait;
        }
        self.pending = None;
        self.manual = false;
        self.force = false;
        if self.attempts >= 5 {
            self.stopped = true;
            return Step::GiveUp;
        }
        self.attempts += 1;
        self.serial = self.serial.wrapping_add(1);
        Step::Retry {
            serial: self.serial,
            attempt: self.attempts,
        }
    }
    pub(crate) fn retry_now(&mut self, now: u64, minimum_delay: u64) {
        self.attempts = 0;
        self.stopped = false;
        self.healthy_since = None;
        self.pending = Some(now.saturating_add(minimum_delay));
        self.manual = true;
    }
    pub(crate) fn manual(&self) -> bool {
        self.manual
    }
    pub(crate) fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn decoder_stuck(&mut self) {
        if self.pending.is_some() {
            self.force = true;
        }
    }
}

/// A connection-local ladder. Never return automatically to a failed primary or format.
#[derive(Clone, Copy, Default)]
pub(crate) struct Ladder {
    alternate: bool,
    backup: bool,
    repaired: bool,
    gaps: u8,
}
#[derive(Debug, PartialEq)]
pub(crate) enum Route {
    Reconnect,
    Alternate,
    Backup,
}
impl Ladder {
    pub(crate) fn skip_gap(&mut self) -> bool {
        if self.gaps >= 2 {
            return false;
        }
        self.gaps += 1;
        true
    }
    pub(crate) fn route(&mut self, attempt: u8, alternate: bool, backup: bool) -> Route {
        if attempt >= 4 && backup && !self.backup {
            self.backup = true;
            return Route::Backup;
        }
        if attempt >= 2 && alternate && !self.alternate && !self.backup {
            self.alternate = true;
            return Route::Alternate;
        }
        Route::Reconnect
    }
    pub(crate) fn repair(&mut self) -> bool {
        if self.repaired {
            return false;
        }
        self.repaired = true;
        true
    }
}

/// Live only: cross a small, genuine buffered hole, not arbitrary missing content.
pub(crate) fn gap_target(position: f64, start: f64, end: f64) -> Option<f64> {
    let gap = start - position;
    (position.is_finite()
        && start.is_finite()
        && end.is_finite()
        && (0.1..=15.0).contains(&gap)
        && end - start >= 3.0)
        .then_some(start + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ladder_changes_format_and_backup_once_without_bouncing() {
        let mut ladder = Ladder::default();
        assert!(ladder.repair());
        assert!(!ladder.repair());
        assert_eq!(ladder.route(1, true, true), Route::Reconnect);
        assert_eq!(ladder.route(2, true, true), Route::Alternate);
        assert_eq!(ladder.route(3, true, true), Route::Reconnect);
        assert_eq!(ladder.route(4, true, true), Route::Backup);
        for _ in 0..100 {
            assert_eq!(ladder.route(5, true, true), Route::Reconnect);
        }
        assert!(std::mem::size_of::<Ladder>() <= 4);
    }
    #[test]
    fn playlist_urls_and_unapproved_backups_are_not_guessed() {
        let mut ladder = Ladder::default();
        for attempt in 1..=5 {
            assert_eq!(ladder.route(attempt, false, false), Route::Reconnect);
        }
        assert_eq!(ladder.route(5, true, true), Route::Backup);
        assert_eq!(ladder.route(5, true, true), Route::Reconnect);
    }
    #[test]
    fn both_buffer_modes_stay_below_the_read_ahead_cap() {
        let mut r = Recovery::default();
        assert_eq!(r.reserve_for(false), 6.0);
        assert_eq!(r.reserve_for(true), 12.0);
        r.attempts = 5;
        assert_eq!(r.reserve_for(false), 18.0);
        assert_eq!(r.reserve_for(true), 24.0);
    }

    #[test]
    fn live_gap_skips_are_small_buffered_and_bounded() {
        assert_eq!(gap_target(10.0, 12.0, 18.0), Some(12.05));
        assert_eq!(gap_target(10.0, 30.0, 40.0), None);
        assert_eq!(gap_target(10.0, 12.0, 13.0), None);
        assert_eq!(gap_target(f64::NAN, 12.0, 18.0), None);
        let mut ladder = Ladder::default();
        assert!(ladder.skip_gap());
        assert!(ladder.skip_gap());
        assert!(!ladder.skip_gap());
    }
    #[test]
    fn repeated_errors_coalesce_and_buffer_is_not_discarded_early() {
        let mut r = Recovery::default();
        assert!(r.request(100, 0, 0));
        assert!(!r.request(200, 0, 0));
        assert_eq!(r.poll(599, false, 0.0), Step::Wait);
        assert_eq!(r.poll(600, false, 8.0), Step::Wait);
        assert_eq!(
            r.poll(700, false, 1.0),
            Step::Retry {
                serial: 1,
                attempt: 1
            }
        );
    }
    #[test]
    fn pauses_cancel_automatic_work() {
        let mut r = Recovery::default();
        r.request(0, 0, 0);
        assert_eq!(r.poll(1000, true, 0.0), Step::Wait);
        assert_eq!(r.poll(2000, false, 0.0), Step::Wait);
    }
    #[test]
    fn retries_are_bounded_and_manual_retry_has_a_fresh_serial() {
        let mut r = Recovery::default();
        for n in 1..=5 {
            r.request(n * 100_000, 0, 0);
            assert_eq!(
                r.poll(n * 100_000 + 9000, false, 0.0),
                Step::Retry {
                    serial: n as u32,
                    attempt: n as u8
                }
            );
        }
        r.request(700_000, 0, 0);
        assert_eq!(r.poll(700_000, false, 0.0), Step::GiveUp);
        assert!(!r.request(800_000, 0, 0));
        r.retry_now(900_000, 0);
        assert_eq!(
            r.poll(900_000, false, 10.0),
            Step::Retry {
                serial: 6,
                attempt: 1
            }
        );
        assert!(std::mem::size_of::<Recovery>() <= 80);
    }
    #[test]
    fn reserve_is_capped_and_health_requires_a_full_minute() {
        let mut r = Recovery {
            attempts: 4,
            ..Default::default()
        };
        assert_eq!(r.reserve(), 18.0);
        r.healthy(0, true);
        r.healthy(15_000, true);
        assert_eq!(r.reserve(), 18.0);
        r.healthy(59_999, true);
        assert_eq!(r.reserve(), 18.0);
        r.healthy(60_000, true);
        assert_eq!(r.reserve(), 6.0);
    }
    fn sample(now: u64) -> Sample {
        Sample {
            now,
            position: 1.0,
            frames: 10,
            has_video: true,
            visible: true,
            ahead: 5.0,
            paused: false,
            holding: false,
            transport: Transport {
                last_data: 0,
                waiting_since: 0,
                reading: true,
                received: 1000,
                ..Default::default()
            },
        }
    }
    #[test]
    fn silence_is_detected_but_backpressure_and_slow_start_are_not_misclassified() {
        let mut m = Monitor::new(0);
        assert!(!m.observe(sample(7999)).stalled);
        assert!(m.observe(sample(8000)).stalled);
        let mut s = sample(100_000);
        s.transport.reading = false;
        assert!(!m.observe(s).stalled);
        let mut s = sample(19_999);
        s.transport.received = 0;
        assert!(!m.observe(s).stalled);
        let mut s = sample(20_000);
        s.transport.received = 0;
        assert!(m.observe(s).stalled);
        let mut s = sample(100_000);
        s.transport.waiting_since = 99_000;
        assert!(!m.observe(s).stalled);
    }
    #[test]
    fn frozen_picture_is_distinguished_from_an_upstream_outage_and_radio() {
        let mut m = Monitor::new(0);
        m.observe(sample(0));
        let mut s = sample(8000);
        s.position = 9.0;
        s.transport.last_data = 8000;
        let health = m.observe(s);
        assert!(health.stuck);
        assert!(!health.stalled);
        assert!(!health.healthy);
        let mut s = sample(9000);
        s.position = 10.0;
        s.has_video = false;
        s.transport.last_data = 9000;
        assert!(!m.observe(s).stuck);
        let mut s = sample(50_000);
        s.paused = true;
        s.transport.last_data = 50_000;
        assert!(!m.observe(s).stuck);
    }
    #[test]
    fn a_stuck_decoder_can_restart_without_waiting_for_its_buffer_to_drain() {
        let mut r = Recovery::default();
        r.request(0, 0, 0);
        r.decoder_stuck();
        assert_eq!(r.poll(499, false, 10.0), Step::Wait);
        assert!(matches!(r.poll(500, false, 10.0), Step::Retry { .. }));
        assert!(std::mem::size_of::<Monitor>() <= 40);
    }
    #[test]
    fn manual_retry_does_not_bypass_an_observed_rate_limit() {
        let mut r = Recovery::default();
        r.retry_now(0, 30_000);
        assert!(r.manual());
        assert_eq!(r.poll(29_999, false, 0.0), Step::Wait);
        assert!(matches!(r.poll(30_000, false, 0.0), Step::Retry { .. }));
    }
    #[test]
    fn background_video_suspension_does_not_trigger_decoder_restarts() {
        let mut m = Monitor::new(0);
        m.observe(sample(0));
        let mut s = sample(100_000);
        s.visible = false;
        s.transport.last_data = 100_000;
        assert!(!m.observe(s).stuck);
        let mut s = sample(100_250);
        s.transport.last_data = 100_250;
        assert!(!m.observe(s).stuck);
    }
    #[test]
    fn observed_retry_after_and_jitter_are_respected() {
        let mut r = Recovery::default();
        r.request(0, 250, 30_000);
        assert_eq!(r.poll(29_999, false, 0.0), Step::Wait);
        assert!(matches!(r.poll(30_000, false, 0.0), Step::Retry { .. }));
    }
}
