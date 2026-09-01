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

#[cfg(not(target_arch = "wasm32"))]
pub struct InstantClock {
    start: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl InstantClock {
    pub fn new() -> Self {
        InstantClock {
            start: std::time::Instant::now(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for InstantClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Clock for InstantClock {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

#[cfg(target_arch = "wasm32")]
pub struct WasmClock;

#[cfg(target_arch = "wasm32")]
impl WasmClock {
    pub fn new() -> Self {
        WasmClock
    }
}

#[cfg(target_arch = "wasm32")]
impl Default for WasmClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
impl Clock for WasmClock {
    fn now_ms(&self) -> u64 {
        js_sys::Date::now() as u64
    }
}
