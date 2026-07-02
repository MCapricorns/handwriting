use super::TextTarget;
use crate::ui::{
    ACCENT, FONT_WEIGHT_SEMIBOLD, TEXT_ON_ACCENT, delete_font, draw_text, fill_rect, ui_font,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Once;
use tracing::debug;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, HGDIOBJ, InvalidateRect, PAINTSTRUCT, SelectObject, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GWLP_USERDATA, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, HWND_TOPMOST,
    MoveWindow, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, TranslateMessage, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONUP, WM_NCCREATE, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const PICKER_WIDTH: i32 = 88;
const PICKER_HEIGHT: i32 = 32;
const PICKER_INSET: i32 = 4;

static REGISTER_PICKER_CLASS: Once = Once::new();

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickerMode {
    Floating,
    Modal,
}

struct PickerState {
    confirmed: Cell<bool>,
    closed: Cell<bool>,
    mode: PickerMode,
}

pub struct TargetPicker {
    hwnd: Option<HWND>,
    target: Option<TextTarget>,
    state: Rc<PickerState>,
}

impl TargetPicker {
    pub fn new() -> Self {
        Self {
            hwnd: None,
            target: None,
            state: Rc::new(PickerState {
                confirmed: Cell::new(false),
                closed: Cell::new(false),
                mode: PickerMode::Floating,
            }),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.hwnd.is_some() && self.target.is_some()
    }

    pub fn show(&mut self, target: &TextTarget) {
        self.state.confirmed.set(false);
        self.state.closed.set(false);
        self.target = Some(target.clone());

        let hwnd = match self.hwnd {
            Some(hwnd) => hwnd,
            None => match create_picker_window(Rc::as_ptr(&self.state).cast()) {
                Ok(hwnd) => {
                    self.hwnd = Some(hwnd);
                    hwnd
                }
                Err(e) => {
                    debug!(?e, "failed to create floating picker window");
                    self.target = None;
                    return;
                }
            },
        };

        let (x, y) = picker_position(&target.rect);
        debug!(?target.rect, x, y, "floating picker position");
        position_picker(hwnd, x, y);
        self.pump_messages();
    }

    pub fn hide(&mut self) {
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        self.state.confirmed.set(false);
        self.state.closed.set(false);
        self.target = None;
    }

    pub fn pump_messages(&mut self) {
        let Some(hwnd) = self.hwnd else {
            return;
        };

        unsafe {
            let mut msg = std::mem::MaybeUninit::uninit();
            while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                msg.as_mut_ptr(),
                Some(hwnd),
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
            )
            .as_bool()
            {
                let msg = msg.assume_init();
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn take_confirmed(&mut self) -> Option<TextTarget> {
        self.pump_messages();
        if !self.state.confirmed.get() {
            return None;
        }
        let target = self.target.take()?;
        self.state.confirmed.set(false);
        self.hide();
        Some(target)
    }

    pub fn confirm_modal(target: &TextTarget) -> bool {
        let state = Rc::new(PickerState {
            confirmed: Cell::new(false),
            closed: Cell::new(false),
            mode: PickerMode::Modal,
        });

        let hwnd = match create_picker_window(Rc::as_ptr(&state).cast()) {
            Ok(hwnd) => hwnd,
            Err(e) => {
                debug!(?e, "failed to create modal picker window");
                return false;
            }
        };

        let (x, y) = picker_position(&target.rect);
        position_picker(hwnd, x, y);
        run_modal_loop(hwnd, &state);

        let confirmed = state.confirmed.get();
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        confirmed
    }
}

pub type FloatingPicker = TargetPicker;

impl Drop for TargetPicker {
    fn drop(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

fn create_picker_window(state_ptr: *const core::ffi::c_void) -> windows::core::Result<HWND> {
    let class_name = windows::core::w!("HandwritingTargetPicker");

    unsafe {
        let instance = GetModuleHandleW(None)?;
        REGISTER_PICKER_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(picker_proc),
                hInstance: instance.into(),
                hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                    None,
                    windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
                )
                .unwrap_or_default(),
                lpszClassName: class_name,
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
        });

        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            windows::core::w!("手写"),
            WS_POPUP | WS_VISIBLE,
            0,
            0,
            PICKER_WIDTH,
            PICKER_HEIGHT,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            Some(state_ptr),
        )
    }
}

fn picker_position(target_rect: &RECT) -> (i32, i32) {
    let mut cursor = POINT::default();
    let _ = unsafe { GetCursorPos(&mut cursor) };

    let target = TextTarget {
        hwnd: HWND::default(),
        rect: *target_rect,
    };

    let (x, y) = if target.is_reasonable_bounds() {
        let x = target_rect.right - PICKER_WIDTH - PICKER_INSET;
        let y_min = target_rect.top + PICKER_INSET;
        let y_max = target_rect.bottom - PICKER_HEIGHT - PICKER_INSET;
        let y = cursor.y.clamp(y_min, y_max.max(y_min));
        (x, y)
    } else {
        (cursor.x + 12, cursor.y - PICKER_HEIGHT / 2)
    };

    clamp_to_virtual_screen(x, y)
}

fn position_picker(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let _ = MoveWindow(hwnd, x, y, PICKER_WIDTH, PICKER_HEIGHT, true);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            PICKER_WIDTH,
            PICKER_HEIGHT,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = UpdateWindow(hwnd);
    }
}

fn clamp_to_virtual_screen(x: i32, y: i32) -> (i32, i32) {
    unsafe {
        let origin_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let origin_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let screen_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let screen_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let max_x = origin_x + screen_w - PICKER_WIDTH;
        let max_y = origin_y + screen_h - PICKER_HEIGHT;
        (
            x.clamp(origin_x, max_x.max(origin_x)),
            y.clamp(origin_y, max_y.max(origin_y)),
        )
    }
}

fn run_modal_loop(hwnd: HWND, state: &PickerState) {
    unsafe {
        let mut msg = std::mem::MaybeUninit::uninit();
        while !state.closed.get() {
            if !GetMessageW(msg.as_mut_ptr(), Some(hwnd), 0, 0).as_bool() {
                break;
            }
            let msg = msg.assume_init();
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "system" fn picker_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let create_struct =
            lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
        if !create_struct.is_null() {
            let state_ptr = (*create_struct).lpCreateParams;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        }
        return LRESULT(1);
    }

    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if state_ptr == 0 {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &*(state_ptr as *const PickerState);

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.0.is_null() {
                let rect = RECT {
                    right: PICKER_WIDTH,
                    bottom: PICKER_HEIGHT,
                    ..Default::default()
                };
                fill_rect(hdc, &rect, ACCENT);
                let font = ui_font(-15, FONT_WEIGHT_SEMIBOLD);
                if !font.0.is_null() {
                    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
                    let label = windows::core::w!("手写");
                    draw_text(hdc, &rect, label.as_wide(), TEXT_ON_ACCENT);
                    SelectObject(hdc, old_font);
                    delete_font(font);
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            state.confirmed.set(true);
            if state.mode == PickerMode::Modal {
                state.closed.set(true);
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            if state.mode == PickerMode::Modal {
                state.closed.set(true);
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
