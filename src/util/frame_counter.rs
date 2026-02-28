use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

pub const EXPECTED_FRAMES_PER_MIN: u64 = 60_000 / 20;
pub struct FrameCounter {
    cur_minute: AtomicI64,
    cur_sent: AtomicU64,
    cur_nulled: AtomicU64,
    pub last_sent: AtomicU64,
    pub last_nulled: AtomicU64,
    playing_since: AtomicI64,
    last_track_started: AtomicI64,
    last_track_ended: AtomicI64,
    last_counted_at: AtomicI64,
}

const ACCEPTABLE_TRACK_SWITCH_MS: i64 = 100;

impl FrameCounter {
    pub fn new() -> Self {
        Self {
            cur_minute: AtomicI64::new(0),
            cur_sent: AtomicU64::new(0),
            cur_nulled: AtomicU64::new(0),
            last_sent: AtomicU64::new(0),
            last_nulled: AtomicU64::new(0),
            playing_since: AtomicI64::new(i64::MAX),
            last_track_started: AtomicI64::new(i64::MAX / 2),
            last_track_ended: AtomicI64::new(i64::MAX),
            last_counted_at: AtomicI64::new(0),
        }
    }

    pub fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn check_minute_rollover(&self) {
        let actual_minute = Self::now_ms() / 60_000;
        let cur = self.cur_minute.load(Ordering::Relaxed);
        if cur != actual_minute {
            self.last_sent
                .store(self.cur_sent.swap(0, Ordering::Relaxed), Ordering::Relaxed);
            self.last_nulled.store(
                self.cur_nulled.swap(0, Ordering::Relaxed),
                Ordering::Relaxed,
            );
            self.cur_minute.store(actual_minute, Ordering::Relaxed);
        }
    }

    pub fn on_periodic(&self, playing: bool) {
        let now = Self::now_ms();
        let last = self.last_counted_at.swap(now, Ordering::Relaxed);
        if last == 0 {
            return;
        }

        let elapsed_ms = (now - last).max(0) as u64;
        let frames = elapsed_ms / 20;
        if frames == 0 {
            return;
        }

        self.check_minute_rollover();
        if playing {
            self.cur_sent.fetch_add(frames, Ordering::Relaxed);
        } else {
            self.cur_nulled.fetch_add(frames, Ordering::Relaxed);
        }
    }

    pub fn on_track_start(&self) {
        let now = Self::now_ms();
        self.last_track_started.store(now, Ordering::Relaxed);
        let ended = self.last_track_ended.load(Ordering::Relaxed);
        let playing_since = self.playing_since.load(Ordering::Relaxed);
        if now - ended > ACCEPTABLE_TRACK_SWITCH_MS || playing_since == i64::MAX {
            self.playing_since.store(now, Ordering::Relaxed);
            self.last_track_ended.store(i64::MAX, Ordering::Relaxed);
        }
    }

    pub fn on_track_end(&self) {
        self.last_track_ended
            .store(Self::now_ms(), Ordering::Relaxed);
    }

    pub fn is_data_usable(&self) -> bool {
        let started = self.last_track_started.load(Ordering::Relaxed);
        let ended = self.last_track_ended.load(Ordering::Relaxed);
        let playing = self.playing_since.load(Ordering::Relaxed);

        if started - ended > ACCEPTABLE_TRACK_SWITCH_MS && ended != i64::MAX {
            return false;
        }

        let last_min_start = (Self::now_ms() / 60_000 - 1) * 60_000;
        playing < last_min_start
    }
}

impl Default for FrameCounter {
    fn default() -> Self {
        Self::new()
    }
}
