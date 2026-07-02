use super::HandwritingManager;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Once;
use tracing::{debug, info, warn};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::RegisterPointerInputTarget;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    IDC_ARROW, LoadCursorW, PT_MOUSE, PT_PEN, RegisterClassW, WM_DESTROY, WM_NCCREATE,
    WM_POINTERDOWN, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::w;

static REGISTER_POINTER_HOST_CLASS: Once = Once::new();

pub struct PointerHost {
    hwnd: HWND,
    _manager: Rc<RefCell<HandwritingManager>>,
}

impl PointerHost {
    pub fn try_start(manager: Rc<RefCell<HandwritingManager>>) -> Option<Self> {
        if let Err(e) = manager.borrow().enable_pointer_delivery() {
            warn!(?e, "POINTER_DELIVERY denied; pen system panel unavailable");
            return None;
        }

        let class_name = w!("HandwritingPointerHost");

        let hwnd = unsafe {
            let instance = match GetModuleHandleW(None) {
                Ok(h) => h,
                Err(e) => {
                    warn!(?e, "GetModuleHandleW failed");
                    manager.borrow().restore_state().ok();
                    return None;
                }
            };

            REGISTER_POINTER_HOST_CLASS.call_once(|| {
                let wc = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(pointer_host_proc),
                    hInstance: instance.into(),
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                    lpszClassName: class_name,
                    ..Default::default()
                };
                let _ = RegisterClassW(&wc);
            });

            match CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("HandwritingPointerHost"),
                WS_POPUP,
                -32000,
                -32000,
                1,
                1,
                None,
                None,
                Some(HINSTANCE(instance.0)),
                Some(Rc::as_ptr(&manager).cast()),
            ) {
                Ok(hwnd) => hwnd,
                Err(e) => {
                    warn!(?e, "pointer host window creation failed");
                    manager.borrow().restore_state().ok();
                    return None;
                }
            }
        };

        if let Err(e) = unsafe { RegisterPointerInputTarget(hwnd, PT_PEN) } {
            warn!(
                ?e,
                "RegisterPointerInputTarget(PT_PEN) denied; pen system panel unavailable"
            );
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            manager.borrow().restore_state().ok();
            return None;
        }

        if let Err(e) = unsafe { RegisterPointerInputTarget(hwnd, PT_MOUSE) } {
            debug!(
                ?e,
                "RegisterPointerInputTarget(PT_MOUSE) denied; mouse uses handwriting button"
            );
        }

        info!("pen system handwriting pointer delivery active");
        Some(Self {
            hwnd,
            _manager: manager,
        })
    }
}

impl Drop for PointerHost {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "system" fn pointer_host_proc(
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
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                state_ptr as isize,
            );
        }
        return LRESULT(1);
    }

    let state_ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if state_ptr == 0 {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let manager = &*(state_ptr as *const RefCell<HandwritingManager>);

    match msg {
        WM_POINTERDOWN => {
            let pointer_id = (wparam.0 & 0xFFFF) as u32;
            debug!(pointer_id, "shell WM_POINTERDOWN");
            match manager.borrow().request_handwriting_for_pointer(pointer_id) {
                Ok(true) => info!(pointer_id, "system handwriting panel opened"),
                Ok(false) => debug!(pointer_id, "system handwriting request declined"),
                Err(e) => warn!(?e, pointer_id, "system handwriting request failed"),
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
