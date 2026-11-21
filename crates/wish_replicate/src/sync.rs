//! Tick sync: keeps a client's simulation ahead of the host by the round
//! trip plus a margin, so that its inputs reach the host before the host
//! simulates their tick (design note `1.1-prediction.md` §3.2).
//!
//! Everything is integer arithmetic on the caller's time (`now`, in
//! microseconds) and on tick numbers: the same observations give the same
//! plan on every machine. Time only ever moves forward: a client that is
//! ahead slows down, it never steps back.

use std::collections::VecDeque;

/// Clock samples kept (one per new newest host tick seen): about half a
/// second at 60 Hz, so a rise in latency shows within that time.
pub const SAMPLES: usize = 32;

/// How long the client waits for the transport's first round-trip
/// measurement before it predicts with the assumed one, in microseconds.
pub const RTT_WAIT_US: u64 = 1_000_000;

/// The settings of tick sync, in the caller's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncSettings {
    /// The length of one tick, in microseconds.
    pub tick_us: u64,
    /// Extra lead on top of the round trip and the jitter, in microseconds.
    pub margin_us: u64,
    /// The round trip assumed before the transport has measured one, in
    /// microseconds.
    pub assumed_rtt_us: u64,
    /// How far behind its target the client may fall before it jumps, in
    /// ticks.
    pub jump_ticks: u64,
    /// Ticks run in one frame at most, outside a jump.
    pub max_steps: u64,
    /// Ticks one jump runs at most.
    pub max_catch_up: u64,
}

/// What tick sync knows and decided, for observation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncState {
    /// The tick the client should have reached now (0 until it knows the
    /// host's clock).
    pub target: u64,
    /// The client's predicted tick minus the newest host tick it has seen.
    pub lead: i64,
    /// The rate of the last frame, in thousandths (1000: real time).
    pub rate_milli: u32,
    /// How far the client was from its target, in thousandths of a tick
    /// (positive: behind).
    pub error_milli: i64,
    /// The host's clock relative to the caller's, in microseconds, as the
    /// fastest recent message shows it.
    pub offset_us: i64,
    /// The mean extra delay of recent messages over the fastest, in
    /// microseconds.
    pub jitter_us: u64,
    /// The round trip used, in microseconds.
    pub rtt_us: u64,
    /// Lead added because the host reported inputs that came late, in
    /// microseconds; it wears off by a tick every 64 frames.
    pub late_us: u64,
    /// Times the host reported a late input.
    pub late_reports: u64,
}

/// What to do this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    /// Ticks to simulate now.
    pub steps: u64,
    /// Whether this is a jump (the client was too far behind).
    pub jumped: bool,
    /// The first tick whose input, sent now, reaches the host before the
    /// host simulates it (the target less the margin); earlier ticks get
    /// the input the host already holds.
    pub in_time_from: u64,
}

/// The tick sync of one client.
#[derive(Debug, Clone)]
pub struct TickSync {
    settings: SyncSettings,
    samples: VecDeque<i64>,
    newest: Option<u64>,
    first_sample_us: Option<u64>,
    last_now_us: Option<u64>,
    owed_milli: u64,
    raised_until: Option<u64>,
    state: SyncState,
}

impl TickSync {
    /// Tick sync with `settings` (a tick of at least one microsecond).
    pub fn new(settings: SyncSettings) -> Self {
        Self {
            settings: SyncSettings {
                tick_us: settings.tick_us.max(1),
                ..settings
            },
            samples: VecDeque::new(),
            newest: None,
            first_sample_us: None,
            last_now_us: None,
            raised_until: None,
            owed_milli: 0,
            state: SyncState {
                rate_milli: 1000,
                ..SyncState::default()
            },
        }
    }

    /// What it knows and decided last.
    pub fn state(&self) -> SyncState {
        self.state
    }

    /// Takes the newest host tick seen at local time `now_us`: a new one is
    /// a sample of the host's clock.
    pub fn observe(&mut self, now_us: u64, newest_seen: Option<u64>) {
        let Some(tick) = newest_seen else {
            return;
        };
        if self.newest.is_some_and(|n| tick <= n) {
            return;
        }
        self.newest = Some(tick);
        self.first_sample_us.get_or_insert(now_us);
        let host_us = i128::from(tick) * i128::from(self.settings.tick_us);
        let sample = clamp_i64(host_us - i128::from(now_us));
        self.samples.push_back(sample);
        while self.samples.len() > SAMPLES {
            self.samples.pop_front();
        }
        let offset = self.samples.iter().copied().max().unwrap_or(sample);
        let spread: i128 = self
            .samples
            .iter()
            .map(|s| i128::from(offset) - i128::from(*s))
            .sum();
        let jitter = spread / self.samples.len().max(1) as i128;
        self.state.offset_us = offset;
        self.state.jitter_us = u64::try_from(jitter).unwrap_or(0);
    }

    /// Takes the host's report that the frame of tick `about` came `ticks`
    /// ticks late (the confirmation of that tick names an older applied
    /// frame), while the client's newest predicted tick is `predicted`: the
    /// lead grows by that much and one tick more, at once. Reports about
    /// frames sent before the last rise (ticks up to the predicted tick of
    /// then) say nothing new and are only counted. The extra lead is at most
    /// one second and wears off by a tick every 64 frames.
    pub fn late(&mut self, ticks: u64, about: u64, predicted: u64) {
        if ticks == 0 {
            return;
        }
        self.state.late_reports += 1;
        if self.raised_until.is_some_and(|until| about <= until) {
            return;
        }
        let extra = ticks
            .saturating_add(1)
            .saturating_mul(self.settings.tick_us);
        self.state.late_us = self.state.late_us.saturating_add(extra).min(1_000_000);
        self.raised_until = Some(predicted);
    }

    /// The host's tick at local time `now_us` as the fastest message would
    /// show it, in thousandths of a tick; `None` before the first sample.
    /// Interpolation draws others a delay behind this.
    pub fn host_clock_milli(&self, now_us: u64) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }
        let at = i128::from(now_us) + i128::from(self.state.offset_us);
        let milli = at * 1000 / i128::from(self.settings.tick_us);
        Some(u64::try_from(milli.max(0)).unwrap_or(u64::MAX))
    }

    /// The plan for the frame at `now_us`, for a client whose newest
    /// predicted tick is `predicted`, with the transport's measured round
    /// trip (`None` before one: the client waits for it up to
    /// [`RTT_WAIT_US`], then assumes one).
    pub fn plan(&mut self, now_us: u64, rtt_us: Option<u64>, predicted: u64) -> Plan {
        let elapsed = self
            .last_now_us
            .map_or(0, |last| now_us.saturating_sub(last));
        self.last_now_us = Some(self.last_now_us.map_or(now_us, |l| l.max(now_us)));
        if let Some(newest) = self.newest {
            self.state.lead = clamp_i64(i128::from(predicted) - i128::from(newest));
        }
        if self.samples.is_empty() {
            return Plan::default();
        }
        let s = self.settings;
        let waited = self
            .first_sample_us
            .is_some_and(|first| now_us.saturating_sub(first) >= RTT_WAIT_US);
        let rtt = match rtt_us {
            Some(rtt) => rtt,
            None if waited => s.assumed_rtt_us,
            None => return Plan::default(),
        };
        self.state.rtt_us = rtt;
        // Late reports wear off slowly: a tick every 64 frames' worth.
        self.state.late_us = self.state.late_us.saturating_sub(elapsed / 64);
        let tick = i128::from(s.tick_us);
        let target_us = i128::from(now_us)
            + i128::from(self.state.offset_us)
            + i128::from(rtt)
            + i128::from(self.state.jitter_us)
            + i128::from(self.state.late_us)
            + i128::from(s.margin_us);
        let target = if target_us <= 0 {
            0
        } else {
            (target_us + tick - 1) / tick
        };
        let target = u64::try_from(target).unwrap_or(u64::MAX);
        self.state.target = target;
        let in_time_from = target.saturating_sub(s.margin_us / s.tick_us);
        let position = i128::from(predicted) * 1000 + i128::from(self.owed_milli);
        let error = i128::from(target) * 1000 - position;
        self.state.error_milli = clamp_i64(error);
        let jump = i128::from(s.jump_ticks) * 1000;
        if error > jump {
            let steps = target.saturating_sub(predicted).min(s.max_catch_up);
            self.owed_milli = 0;
            self.state.rate_milli = 1000;
            return Plan {
                steps,
                jumped: true,
                in_time_from,
            };
        }
        // Behind: up to a quarter faster (inputs may already be late).
        // Ahead: a tenth slower, half when far ahead; never stopped.
        let change = if error >= 0 {
            (error / 8).min(250)
        } else if error < -jump {
            (error / 20).max(-500)
        } else {
            (error / 20).max(-100)
        };
        let rate = (1000 + change) as u64;
        self.state.rate_milli = rate as u32;
        let owed = u128::from(self.owed_milli)
            + u128::from(elapsed) * u128::from(rate) / u128::from(s.tick_us);
        let steps = u64::try_from(owed / 1000)
            .unwrap_or(u64::MAX)
            .min(s.max_steps);
        let rest = owed - u128::from(steps) * 1000;
        // Never bank more than one tick: a long frame is not paid back later
        // in a burst.
        self.owed_milli = u64::try_from(rest.min(999)).unwrap_or(999);
        Plan {
            steps,
            jumped: false,
            in_time_from,
        }
    }
}

fn clamp_i64(value: i128) -> i64 {
    i64::try_from(value.clamp(i128::from(i64::MIN), i128::from(i64::MAX))).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> SyncSettings {
        SyncSettings {
            tick_us: 16_000,
            margin_us: 32_000,
            assumed_rtt_us: 100_000,
            jump_ticks: 8,
            max_steps: 4,
            max_catch_up: 120,
        }
    }

    /// A host ticking every 16 ms whose messages take `delay` ms to arrive;
    /// a client that runs the plan every 16 ms. Returns the client's
    /// predicted tick per frame and the lead at the end.
    fn run(delay_ms: u64, rtt_ms: u64, frames: u64) -> (Vec<u64>, SyncState) {
        let mut sync = TickSync::new(settings());
        let mut predicted = 0;
        let mut ticks = Vec::new();
        for frame in 1..=frames {
            let now_us = frame * 16_000;
            let seen = (frame * 16).saturating_sub(delay_ms) / 16;
            sync.observe(now_us, Some(seen));
            let plan = sync.plan(now_us, Some(rtt_ms * 1000), predicted);
            predicted += plan.steps;
            ticks.push(predicted);
        }
        (ticks, sync.state())
    }

    #[test]
    fn the_lead_settles_at_the_round_trip_plus_the_margin_and_never_goes_back() {
        let (ticks, state) = run(96, 192, 600);
        assert!(ticks.windows(2).all(|w| w[1] >= w[0]));
        // Round trip 12 ticks + margin 2 ticks; the host is 6 ticks ahead of
        // what the client sees, so the input arrives with two ticks to spare.
        assert!(
            (13..=16).contains(&state.lead),
            "lead {} ticks ({state:?})",
            state.lead
        );
        // Settled: one tick per frame, at real time.
        let tail: Vec<u64> = ticks[500..].windows(2).map(|w| w[1] - w[0]).collect();
        assert!(tail.iter().all(|d| *d == 1), "{tail:?}");
    }

    #[test]
    fn a_rise_in_latency_is_caught_up_and_a_fall_only_slows_down() {
        let mut sync = TickSync::new(settings());
        let mut predicted = 0u64;
        let mut previous = 0u64;
        let mut rates = Vec::new();
        for frame in 1..=900u64 {
            let delay = if frame < 300 {
                48
            } else if frame < 600 {
                160
            } else {
                32
            };
            let now_us = frame * 16_000;
            let seen = (frame * 16).saturating_sub(delay) / 16;
            sync.observe(now_us, Some(seen));
            let plan = sync.plan(now_us, Some(delay * 2000), predicted);
            predicted += plan.steps;
            assert!(predicted >= previous);
            previous = predicted;
            rates.push(sync.state().rate_milli);
        }
        // Slowing down never goes below half speed, and never stops.
        assert!(rates.iter().all(|r| (500..=1250).contains(r)), "{rates:?}");
        let state = sync.state();
        assert!((3..=8).contains(&state.lead), "{state:?}");
    }

    #[test]
    fn a_far_target_is_a_jump_forward() {
        let mut sync = TickSync::new(settings());
        sync.observe(1_000_000, Some(100));
        let plan = sync.plan(1_000_000, Some(200_000), 100);
        assert!(plan.jumped);
        assert!(plan.steps > 8);
        assert_eq!(sync.state().target, 100 + plan.steps);
    }

    #[test]
    fn nothing_runs_before_the_host_clock_is_known() {
        let mut sync = TickSync::new(settings());
        assert_eq!(sync.plan(16_000, None, 0), Plan::default());
        assert_eq!(sync.host_clock_milli(16_000), None);
        // The clock is known, the round trip not yet: wait for it, then
        // assume one.
        sync.observe(32_000, Some(2));
        assert_eq!(sync.plan(48_000, None, 2), Plan::default());
        assert!(sync.plan(32_000 + RTT_WAIT_US, None, 2).steps > 0);
    }

    #[test]
    fn a_late_report_raises_the_lead_at_once_and_wears_off() {
        let mut sync = TickSync::new(settings());
        sync.observe(160_000, Some(10));
        let _ = sync.plan(160_000, Some(32_000), 10);
        let before = sync.state().target;
        sync.late(3, 10, 10);
        // A report about a frame sent before that rise adds nothing.
        sync.late(5, 9, 10);
        let _ = sync.plan(176_000, Some(32_000), 10);
        assert_eq!(sync.state().late_us, 4 * 16_000 - 250);
        assert!(sync.state().target >= before + 4);
        // A frame sent after it that is late again raises it again.
        sync.late(1, 11, 14);
        assert_eq!(sync.state().late_us, 6 * 16_000 - 250);
        for frame in 0..400u64 {
            let _ = sync.plan(192_000 + frame * 16_000, Some(32_000), 10);
        }
        assert_eq!(sync.state().late_us, 0);
        assert_eq!(sync.state().late_reports, 3);
    }
}
