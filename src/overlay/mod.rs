mod session;
mod window;

pub const HOTKEY_SUBMIT: i32 = 2;
pub const HOTKEY_CANCEL: i32 = 3;

pub use session::OverlaySession;
pub use window::{OverlayAction, OverlayConfig, OverlayWindow};
