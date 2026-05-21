use cef::Browser;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};

/// Global mutable state shared between the stdin reader thread and the UI-thread tasks.
pub struct HostState {
    pub next_browser_id: u32,
    pub browsers: HashMap<u32, Browser>,
    /// FIFO of request ids that called `open` and are waiting for `on_after_created`
    /// to assign a `browser_id` and reply.
    pub pending_opens: VecDeque<u64>,
    /// True after `shutdown` was requested — the next `on_before_close` will quit the loop.
    pub shutting_down: bool,
}

impl HostState {
    fn new() -> Self {
        Self {
            next_browser_id: 0,
            browsers: HashMap::new(),
            pending_opens: VecDeque::new(),
            shutting_down: false,
        }
    }

    pub fn allocate_browser_id(&mut self) -> u32 {
        self.next_browser_id = self.next_browser_id.wrapping_add(1);
        // Skip 0 to keep it unambiguous.
        if self.next_browser_id == 0 {
            self.next_browser_id = 1;
        }
        self.next_browser_id
    }
}

static HOST_STATE: OnceLock<Arc<Mutex<HostState>>> = OnceLock::new();

pub fn host_state() -> Arc<Mutex<HostState>> {
    HOST_STATE
        .get_or_init(|| Arc::new(Mutex::new(HostState::new())))
        .clone()
}
