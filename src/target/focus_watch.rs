use super::TextTarget;
use std::time::{Duration, Instant};

const STABLE_FOCUS_DELAY: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

pub struct FocusWatch {
    stable_since: Option<Instant>,
    last_hwnd: Option<isize>,
    triggered_hwnd: Option<isize>,
}

impl FocusWatch {
    pub fn new() -> Self {
        Self {
            stable_since: None,
            last_hwnd: None,
            triggered_hwnd: None,
        }
    }

    pub fn poll_interval() -> Duration {
        POLL_INTERVAL
    }

    pub fn poll(&mut self, current: Option<TextTarget>) -> Option<TextTarget> {
        let target = current?;
        let hwnd_key = target.hwnd.0 as isize;

        if self.last_hwnd != Some(hwnd_key) {
            self.last_hwnd = Some(hwnd_key);
            self.stable_since = Some(Instant::now());
            return None;
        }

        if self.triggered_hwnd == Some(hwnd_key) {
            return None;
        }

        let stable_since = self.stable_since?;
        if stable_since.elapsed() < STABLE_FOCUS_DELAY {
            return None;
        }

        self.triggered_hwnd = Some(hwnd_key);
        Some(target)
    }

    pub fn clear_trigger(&mut self) {
        self.triggered_hwnd = None;
        self.stable_since = None;
        self.last_hwnd = None;
    }

    pub fn reset_if_focus_changed(&mut self, hwnd: Option<isize>) {
        if self.last_hwnd != hwnd {
            self.triggered_hwnd = None;
            self.last_hwnd = hwnd;
            self.stable_since = hwnd.map(|_| Instant::now());
        }
    }
}
