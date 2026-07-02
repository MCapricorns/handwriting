mod injection;
mod ink;
mod overlay;
mod shell_handwriting;
mod target;
mod ui;

use anyhow::{Context, Result};
use injection::inject_text_target;
use ink::{InkRecognizerService, recognize_off_thread};
use overlay::{OverlayAction, OverlayConfig, OverlaySession};
use shell_handwriting::{HandwritingManager, PointerHost};
use std::cell::RefCell;
use std::rc::Rc;
use std::thread;
use target::{FloatingPicker, FocusWatch, TextTarget, detect_text_target, pick_text_target};
use tracing::{info, warn};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::WinRT::RO_INIT_MULTITHREADED;
use windows::Win32::System::WinRT::RoInitialize;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, VK_H,
};
use windows::Win32::UI::Input::Pointer::EnableMouseInPointer;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_HOTKEY, WM_QUIT,
};

const HOTKEY_CUSTOM: i32 = 1;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();
        let _ = RoInitialize(RO_INIT_MULTITHREADED).ok();
        EnableMouseInPointer(true).context("EnableMouseInPointer failed")?;
    }

    let shell_mode = std::env::args().any(|arg| arg == "--shell");
    run_app(shell_mode)
}

fn run_app(shell_mode: bool) -> Result<()> {
    let _ = InkRecognizerService::new().context("failed to initialize Windows Ink recognizer")?;

    let handwriting_manager = HandwritingManager::new()
        .ok()
        .map(|manager| Rc::new(RefCell::new(manager)));

    if shell_mode {
        match handwriting_manager.as_ref() {
            Some(manager) => {
                let state = manager
                    .borrow()
                    .current_state()
                    .context("failed to read handwriting state")?;
                info!(state = state.0, "shell handwriting mode");

                if PointerHost::try_start(Rc::clone(manager)).is_some() {
                    info!("pen: system handwriting panel enabled");
                } else {
                    warn!(
                        "pen path unavailable (access denied is common); mouse button still works"
                    );
                }
                info!("shell (--shell): focus input 2s for handwriting button, or Win+Shift+H");
            }
            None => {
                warn!("ITfHandwriting unavailable, running overlay-only mode");
            }
        }
    } else {
        info!("handwriting overlay mode (default)");
        info!("focus an input field for 2 seconds to show the button, or press Win+Shift+H");
    }

    run_interactive_loop(handwriting_manager.as_ref())
}

fn run_interactive_loop(
    handwriting_manager: Option<&Rc<RefCell<HandwritingManager>>>,
) -> Result<()> {
    register_custom_hotkey()?;

    let mut focus_watch = FocusWatch::new();
    let mut floating_picker = FloatingPicker::new();

    loop {
        let detected = detect_text_target();
        let focused_hwnd = detected.as_ref().map(|t| t.hwnd.0 as isize);
        focus_watch.reset_if_focus_changed(focused_hwnd);
        if focused_hwnd.is_none() && floating_picker.is_visible() {
            floating_picker.hide();
        }

        let mut msg = MSG::default();
        let mut had_message = false;
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                had_message = true;
                if msg.message == WM_QUIT {
                    return Ok(());
                }

                if msg.message == WM_HOTKEY && msg.wParam.0 == HOTKEY_CUSTOM as usize {
                    floating_picker.hide();
                    if let Err(e) = run_custom_session(handwriting_manager, None) {
                        warn!(?e, "handwriting session failed");
                    }
                    focus_watch.clear_trigger();
                    continue;
                }

                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if let Some(target) = focus_watch.poll(detected) {
            info!(
                hwnd = ?target.hwnd,
                rect = ?target.rect,
                "showing handwriting button after stable focus"
            );
            floating_picker.show(&target);
        } else {
            floating_picker.pump_messages();
        }

        if let Some(target) = floating_picker.take_confirmed() {
            if let Err(e) = run_custom_session(handwriting_manager, Some(target)) {
                warn!(?e, "handwriting session failed");
            }
            focus_watch.clear_trigger();
        }

        if !had_message {
            thread::sleep(FocusWatch::poll_interval());
        }
    }
}

fn register_custom_hotkey() -> Result<()> {
    unsafe {
        RegisterHotKey(
            None,
            HOTKEY_CUSTOM,
            MOD_WIN | MOD_SHIFT | MOD_NOREPEAT,
            VK_H.0 as u32,
        )
        .context("failed to register Win+Shift+H hotkey for custom mode")?;
    }
    Ok(())
}

fn run_custom_session(
    handwriting_manager: Option<&Rc<RefCell<HandwritingManager>>>,
    preset_target: Option<TextTarget>,
) -> Result<()> {
    let text_target = match preset_target {
        Some(target) => target,
        None => match pick_text_target() {
            Some(target) => target,
            None => {
                info!("no input box detected; click inside an edit field or hover over it");
                return Ok(());
            }
        },
    };

    info!(hwnd = ?text_target.hwnd, "confirmed text target");

    if let Some(manager) = handwriting_manager {
        manager
            .borrow()
            .suspend_system_handwriting()
            .context("failed to suspend system shell handwriting")?;
    }

    let session = OverlaySession::open(OverlayConfig::translucent())
        .context("failed to create fullscreen overlay")?;

    let action = session.run();

    if let Some(manager) = handwriting_manager {
        manager
            .borrow()
            .restore_state()
            .context("failed to restore handwriting state")?;
    }

    match action {
        OverlayAction::Submit { strokes } => {
            if strokes.is_empty() {
                info!("no ink strokes captured");
                return Ok(());
            }

            let candidates = recognize_off_thread(strokes).context("recognition failed")?;
            if let Some(best) = candidates.first() {
                info!(text = %best.text, "recognized text");
                inject_text_target(&text_target, &best.text)
                    .context("failed to inject text into target app")?;
            } else {
                warn!("no recognition result");
            }
        }
        OverlayAction::Cancel => {
            info!("handwriting session cancelled");
        }
    }

    Ok(())
}
