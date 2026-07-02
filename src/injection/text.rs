use crate::target::TextTarget;
use tracing::{info, warn};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, Sleep};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput, SetFocus, VIRTUAL_KEY,
    VK_CONTROL, VK_MENU, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, BringWindowToTop, EnumChildWindows, GetClassNameW,
    GetForegroundWindow, GetParent, GetWindowThreadProcessId, IsWindowVisible, SW_SHOW,
    SendMessageW, SetCursorPos, SetForegroundWindow, ShowWindow, WM_PASTE,
};
use windows_core::BOOL;

const E_FAIL: i32 = 0x80004005u32 as i32;

type InjectionStrategy = fn(&TextTarget, HWND, &str) -> windows::core::Result<()>;

pub fn inject_text_target(target: &TextTarget, text: &str) -> windows::core::Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let top = top_level_window(target.hwnd);
    try_activate_target_window(top);

    let strategies: &[(&str, InjectionStrategy)] = &[
        ("standard Edit + SendInput", inject_via_edit_sendinput),
        ("UIA focus + SendInput", inject_via_uia_sendinput),
        ("click + clipboard paste", inject_via_click_paste),
    ];

    for (name, strategy) in strategies {
        match strategy(target, top, text) {
            Ok(()) => {
                info!(strategy = name, "injected text");
                return Ok(());
            }
            Err(e) => warn!(strategy = name, ?e, "injection strategy failed"),
        }
    }

    Err(windows::core::Error::new(
        windows::core::HRESULT(E_FAIL),
        "all injection strategies failed",
    ))
}

fn inject_via_edit_sendinput(
    target: &TextTarget,
    top: HWND,
    text: &str,
) -> windows::core::Result<()> {
    let edit_hwnd = find_standard_edit_hwnd(target).ok_or_else(windows::core::Error::from_win32)?;
    unsafe {
        let _ = SetFocus(Some(edit_hwnd));
        Sleep(100);
    }
    send_unicode_input_to_window(top, text)
}

fn inject_via_uia_sendinput(
    target: &TextTarget,
    top: HWND,
    text: &str,
) -> windows::core::Result<()> {
    focus_target_point(target)?;
    send_unicode_input_to_window(top, text)
}

fn inject_via_click_paste(target: &TextTarget, top: HWND, text: &str) -> windows::core::Result<()> {
    focus_target_point(target)?;

    if !is_foreground(top) {
        warn!(?top, current = ?unsafe { GetForegroundWindow() }, "target not foreground before paste");
        try_activate_target_window(top);
        focus_target_point(target)?;
    }

    set_clipboard_text(text)?;
    send_ctrl_v_to_window(top)?;
    unsafe {
        Sleep(50);
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
            let _ = SendMessageW(fg, WM_PASTE, None, None);
        }
    }
    Ok(())
}

fn focus_target_point(target: &TextTarget) -> windows::core::Result<()> {
    let point = center_point(&target.rect);
    click_screen_point(point)?;
    unsafe {
        Sleep(120);
    }
    focus_uia_at_point(point)?;
    unsafe {
        Sleep(120);
    }
    Ok(())
}

fn focus_uia_at_point(point: POINT) -> windows::core::Result<()> {
    unsafe {
        let automation =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let element = automation.ElementFromPoint(point)?;
        element.SetFocus()?;
    }
    Ok(())
}

fn try_activate_target_window(top: HWND) {
    if top.0.is_null() {
        return;
    }

    for _ in 0..6 {
        let _ = try_force_foreground(top);
        unsafe {
            let _ = BringWindowToTop(top);
            let _ = ShowWindow(top, SW_SHOW);
            Sleep(60);
        }
        if is_foreground(top) {
            return;
        }
    }

    warn!(?top, current = ?unsafe { GetForegroundWindow() }, "could not activate target window");
}

fn is_foreground(hwnd: HWND) -> bool {
    unsafe { GetForegroundWindow() == hwnd }
}

fn try_force_foreground(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }

    unsafe {
        let mut process_id = 0u32;
        let target_tid = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        let _ = AllowSetForegroundWindow(process_id);
        let _ = AllowSetForegroundWindow(ASFW_ANY);

        if GetForegroundWindow() == hwnd {
            return true;
        }

        let foreground = GetForegroundWindow();
        let foreground_tid = GetWindowThreadProcessId(foreground, None);
        let self_tid = GetCurrentThreadId();

        let attached_fg = if foreground_tid != 0 && foreground_tid != self_tid {
            AttachThreadInput(self_tid, foreground_tid, true).as_bool()
        } else {
            false
        };

        let attached_target = if target_tid != self_tid {
            AttachThreadInput(self_tid, target_tid, true).as_bool()
        } else {
            false
        };

        let mut ok = SetForegroundWindow(hwnd).as_bool();
        if !ok {
            let alt = [key_event(VK_MENU, false), key_event(VK_MENU, true)];
            let _ = SendInput(&alt, std::mem::size_of::<INPUT>() as i32);
            ok = SetForegroundWindow(hwnd).as_bool();
        }

        if attached_target {
            let _ = AttachThreadInput(self_tid, target_tid, false);
        }
        if attached_fg {
            let _ = AttachThreadInput(self_tid, foreground_tid, false);
        }

        ok
    }
}

fn find_standard_edit_hwnd(target: &TextTarget) -> Option<HWND> {
    if is_standard_edit_hwnd(target.hwnd) {
        return Some(target.hwnd);
    }

    let top = top_level_window(target.hwnd);
    let child = resolve_input_hwnd(top);
    if is_standard_edit_hwnd(child) {
        Some(child)
    } else {
        None
    }
}

fn is_standard_edit_hwnd(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }

    let mut class_name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut class_name) };
    if len == 0 {
        return false;
    }

    let name = String::from_utf16_lossy(&class_name[..len as usize]);
    matches!(
        name.as_str(),
        "Edit" | "RICHEDIT50W" | "RichEditD2DPT" | "RichEdit20W" | "RichEdit20A"
    )
}

fn top_level_window(hwnd: HWND) -> HWND {
    if hwnd.0.is_null() {
        return hwnd;
    }

    let mut current = hwnd;
    loop {
        let parent = unsafe { GetParent(current) };
        match parent {
            Ok(parent) if !parent.0.is_null() => current = parent,
            _ => break,
        }
    }
    current
}

fn set_clipboard_text(text: &str) -> windows::core::Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();

    for _ in 0..5 {
        let opened = unsafe { OpenClipboard(None).is_ok() };
        if !opened {
            unsafe {
                Sleep(30);
            }
            continue;
        }

        let result = (|| unsafe {
            let _ = EmptyClipboard();
            let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
            let locked = GlobalLock(hglobal);
            if locked.is_null() {
                return Err(windows::core::Error::from_win32());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), locked.cast::<u16>(), wide.len());
            let _ = GlobalUnlock(hglobal);
            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hglobal.0)))?;
            Ok(())
        })();

        let _ = unsafe { CloseClipboard() };
        return result;
    }

    Err(windows::core::Error::from_win32())
}

fn send_ctrl_v_to_window(top: HWND) -> windows::core::Result<()> {
    let inputs = [
        key_event(VK_CONTROL, false),
        key_event(VK_V, false),
        key_event(VK_V, true),
        key_event(VK_CONTROL, true),
    ];
    send_input_to_window(top, &inputs)
}

fn send_unicode_input_to_window(top: HWND, text: &str) -> windows::core::Result<()> {
    for ch in text.chars() {
        let code = ch as u16;
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: code,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: code,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        send_input_to_window(top, &inputs)?;
    }
    Ok(())
}

fn send_input_to_window(top: HWND, inputs: &[INPUT]) -> windows::core::Result<()> {
    unsafe {
        let target_tid = GetWindowThreadProcessId(top, None);
        let self_tid = GetCurrentThreadId();
        let attached = if target_tid != self_tid {
            AttachThreadInput(self_tid, target_tid, true).as_bool()
        } else {
            false
        };

        let sent = SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
        if attached {
            let _ = AttachThreadInput(self_tid, target_tid, false);
        }

        if sent == inputs.len() as u32 {
            Ok(())
        } else {
            warn!(sent, expected = inputs.len(), "SendInput count mismatch");
            Err(windows::core::Error::from_win32())
        }
    }
}

fn click_screen_point(point: POINT) -> windows::core::Result<()> {
    unsafe {
        SetCursorPos(point.x, point.y)?;
        Sleep(30);
        let inputs = [
            mouse_button(MOUSEEVENTF_LEFTDOWN),
            mouse_button(MOUSEEVENTF_LEFTUP),
        ];
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent == inputs.len() as u32 {
            Ok(())
        } else {
            Err(windows::core::Error::from_win32())
        }
    }
}

fn center_point(rect: &RECT) -> POINT {
    POINT {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    }
}

fn mouse_button(flag: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flag,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key_event(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn resolve_input_hwnd(hwnd: HWND) -> HWND {
    if hwnd.0.is_null() {
        return hwnd;
    }

    let mut found = HWND::default();
    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(enum_input_child),
            LPARAM(&mut found as *mut HWND as isize),
        );
    }

    if !found.0.is_null() { found } else { hwnd }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "system" fn enum_input_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    if is_standard_edit_hwnd(hwnd) {
        let found = &mut *(lparam.0 as *mut HWND);
        *found = hwnd;
        return BOOL(0);
    }

    BOOL(1)
}
