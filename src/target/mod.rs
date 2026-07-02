mod detect;
mod focus_watch;
mod picker;

pub use detect::{TextTarget, detect_text_target};
pub use focus_watch::FocusWatch;
pub use picker::{FloatingPicker, TargetPicker};

pub fn pick_text_target() -> Option<TextTarget> {
    let target = detect_text_target()?;
    if TargetPicker::confirm_modal(&target) {
        Some(target)
    } else {
        None
    }
}
