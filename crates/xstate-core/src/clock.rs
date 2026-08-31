use std::time::Instant;

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct MockClock {
    pub now: u64,
}

impl MockClock {
    pub fn new(start: u64) -> Self {
        MockClock { now: start }
    }

    pub fn advance(&mut self, ms: u64) {
        self.now += ms;
    }
}

impl Default for MockClock {
    fn default() -> Self {
        MockClock::new(0)
    }
}

impl Clock for MockClock {
    fn now_ms(&self) -> u64 {
        self.now
    }
}

pub struct InstantClock {
    start: Instant,
}

impl InstantClock {
    pub fn new() -> Self {
        InstantClock {
            start: Instant::now(),
        }
    }
}

impl Default for InstantClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for InstantClock {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}
