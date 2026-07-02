use super::{HOTKEY_CANCEL, HOTKEY_SUBMIT, OverlayAction, OverlayConfig, OverlayWindow};
use anyhow::{Context, Result};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_ESCAPE, VK_RETURN,
};

pub struct OverlaySession {
    overlay: OverlayWindow,
}

impl OverlaySession {
    pub fn open(config: OverlayConfig) -> Result<Self> {
        register_session_hotkeys()?;
        let overlay = OverlayWindow::show(config).context("failed to create overlay window")?;
        Ok(Self { overlay })
    }

    pub fn run(&self) -> OverlayAction {
        self.overlay.run_message_loop()
    }
}

impl Drop for OverlaySession {
    fn drop(&mut self) {
        unregister_session_hotkeys();
    }
}

fn register_session_hotkeys() -> Result<()> {
    unsafe {
        RegisterHotKey(None, HOTKEY_SUBMIT, MOD_NOREPEAT, VK_RETURN.0 as u32)
            .context("failed to register Enter hotkey")?;
        RegisterHotKey(None, HOTKEY_CANCEL, MOD_NOREPEAT, VK_ESCAPE.0 as u32)
            .context("failed to register Escape hotkey")?;
    }
    Ok(())
}

fn unregister_session_hotkeys() {
    unsafe {
        let _ = UnregisterHotKey(None, HOTKEY_SUBMIT);
        let _ = UnregisterHotKey(None, HOTKEY_CANCEL);
    }
}
