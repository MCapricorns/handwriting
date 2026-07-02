use crate::ink::InkPoint;
use crate::overlay::{HOTKEY_CANCEL, HOTKEY_SUBMIT};

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Once;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HBRUSH, HGDIOBJ,
    InvalidateRect, MoveToEx, PAINTSTRUCT, PS_SOLID, PolylineTo, ScreenToClient, SelectObject,
    UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::RegisterPointerInputTarget;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::Input::Pointer::{
    GetPointerFrameInfo, GetPointerFramePenInfo, GetPointerInfo, GetPointerInfoHistory,
    POINTER_FLAG_CONFIDENCE, POINTER_INFO, POINTER_PEN_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GWLP_USERDATA, GetClientRect, GetSystemMetrics, GetWindowLongPtrW, HTCLIENT, IDC_ARROW,
    KillTimer, LWA_ALPHA, LoadCursorW, PM_REMOVE, PT_MOUSE, PT_PEN, PT_TOUCH, PeekMessageW,
    PostMessageW, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_SHOW, SetCursor, SetForegroundWindow, SetLayeredWindowAttributes,
    SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage, WM_DESTROY, WM_ERASEBKGND,
    WM_HOTKEY, WM_KEYDOWN, WM_NCCREATE, WM_NCHITTEST, WM_PAINT, WM_POINTERDOWN, WM_POINTERUP,
    WM_POINTERUPDATE, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WaitMessage,
};
use windows::core::w;

const WM_OVERLAY_WAKE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1;
const OVERLAY_ALPHA: u8 = 210;
const IDLE_SUBMIT: Duration = Duration::from_secs(2);
const TIMER_IDLE_CHECK: usize = 1;

static REGISTER_OVERLAY_CLASS: Once = Once::new();

#[derive(Clone, Debug)]
pub enum OverlayAction {
    Submit { strokes: Vec<Vec<InkPoint>> },
    Cancel,
}

#[derive(Clone, Debug)]
pub struct OverlayConfig {
    pub background_color: COLORREF,
    pub ink_color: COLORREF,
    pub ink_width: i32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self::translucent()
    }
}

impl OverlayConfig {
    pub fn translucent() -> Self {
        Self {
            background_color: COLORREF(0x00F5F3EE),
            ink_color: COLORREF(0x001A1A2E),
            ink_width: 5,
        }
    }
}

struct OverlayState {
    strokes: Vec<Vec<InkPoint>>,
    current_stroke: Vec<InkPoint>,
    active_pointer: Option<u32>,
    preview_stroke: Vec<InkPoint>,
    last_activity: Option<Instant>,
    config: OverlayConfig,
    action: Option<OverlayAction>,
}

pub struct OverlayWindow {
    hwnd: HWND,
    state: Rc<RefCell<OverlayState>>,
}

impl OverlayWindow {
    pub fn show(config: OverlayConfig) -> windows::core::Result<Self> {
        let state = Rc::new(RefCell::new(OverlayState {
            strokes: Vec::new(),
            current_stroke: Vec::new(),
            active_pointer: None,
            preview_stroke: Vec::new(),
            last_activity: None,
            config,
            action: None,
        }));

        let class_name = w!("HandwritingOverlay");

        unsafe {
            let instance = GetModuleHandleW(None)?;
            REGISTER_OVERLAY_CLASS.call_once(|| {
                let wc = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance.into(),
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                    lpszClassName: class_name,
                    ..Default::default()
                };
                let _ = RegisterClassW(&wc);
            });

            let origin_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let origin_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                class_name,
                w!("手写输入"),
                WS_POPUP,
                origin_x,
                origin_y,
                width,
                height,
                None,
                None,
                Some(HINSTANCE(instance.0)),
                Some(Rc::as_ptr(&state).cast()),
            )?;

            SetLayeredWindowAttributes(hwnd, COLORREF(0), OVERLAY_ALPHA, LWA_ALPHA)?;
            let _ = SetTimer(Some(hwnd), TIMER_IDLE_CHECK, 100, None);
            let _ = RegisterPointerInputTarget(hwnd, PT_MOUSE);
            let _ = RegisterPointerInputTarget(hwnd, PT_PEN);
            let _ = RegisterPointerInputTarget(hwnd, PT_TOUCH);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = UpdateWindow(hwnd);

            Ok(Self { hwnd, state })
        }
    }

    pub fn run_message_loop(&self) -> OverlayAction {
        unsafe {
            let mut msg =
                std::mem::MaybeUninit::<windows::Win32::UI::WindowsAndMessaging::MSG>::uninit();
            loop {
                if self.state.borrow().action.is_some() {
                    break;
                }

                if PeekMessageW(msg.as_mut_ptr(), None, 0, 0, PM_REMOVE).as_bool() {
                    let msg = msg.assume_init();
                    if msg.message == WM_HOTKEY {
                        match msg.wParam.0 as i32 {
                            id if id == HOTKEY_SUBMIT => {
                                finish_session(
                                    self.hwnd,
                                    &self.state,
                                    build_submit_action(&self.state),
                                );
                            }
                            id if id == HOTKEY_CANCEL => {
                                finish_session(self.hwnd, &self.state, OverlayAction::Cancel);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if msg.hwnd != self.hwnd {
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                } else {
                    let _ = WaitMessage();
                }
            }
        }

        self.state
            .borrow()
            .action
            .clone()
            .unwrap_or(OverlayAction::Cancel)
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn finish_session(hwnd: HWND, state: &RefCell<OverlayState>, action: OverlayAction) {
    state.borrow_mut().action = Some(action);
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_OVERLAY_WAKE, WPARAM(0), LPARAM(0));
    }
}

fn build_submit_action(state: &RefCell<OverlayState>) -> OverlayAction {
    let mut s = state.borrow_mut();
    if !s.current_stroke.is_empty() {
        let stroke = s.current_stroke.clone();
        s.strokes.push(stroke);
        s.current_stroke.clear();
    }
    OverlayAction::Submit {
        strokes: s.strokes.clone(),
    }
}

fn touch_activity(state: &mut OverlayState) {
    state.last_activity = Some(Instant::now());
}

fn commit_current_stroke(state: &mut OverlayState) {
    if state.current_stroke.len() >= 2 {
        let stroke = state.current_stroke.clone();
        state.strokes.push(stroke);
    }
    state.current_stroke.clear();
    state.preview_stroke.clear();
}

fn points_differ(a: &InkPoint, b: &InkPoint) -> bool {
    (a.x - b.x).abs() > 0.5 || (a.y - b.y).abs() > 0.5
}

fn append_points(stroke: &mut Vec<InkPoint>, points: &[InkPoint]) {
    for point in points {
        if stroke.last().is_none_or(|last| points_differ(last, point)) {
            stroke.push(*point);
        }
    }
}

fn is_committed_pointer_point(info: &POINTER_INFO) -> bool {
    if info.pointerFlags.contains(POINTER_FLAG_CONFIDENCE) {
        return true;
    }
    matches!(info.pointerType, PT_MOUSE | PT_TOUCH)
}

fn pointer_info_to_point(hwnd: HWND, info: &POINTER_INFO) -> Option<InkPoint> {
    unsafe {
        let mut pt = info.ptPixelLocation;
        if !ScreenToClient(hwnd, &mut pt).as_bool() {
            return None;
        }
        Some(InkPoint {
            x: pt.x as f32,
            y: pt.y as f32,
        })
    }
}

fn pointer_frame_points(hwnd: HWND, pointer_id: u32) -> Vec<(InkPoint, bool)> {
    unsafe {
        let mut count = 0u32;
        if GetPointerFramePenInfo(pointer_id, &mut count, None).is_ok() && count > 0 {
            let mut infos = vec![POINTER_PEN_INFO::default(); count as usize];
            if GetPointerFramePenInfo(pointer_id, &mut count, Some(infos.as_mut_ptr())).is_ok() {
                return infos[..count as usize]
                    .iter()
                    .filter_map(|info| {
                        pointer_info_to_point(hwnd, &info.pointerInfo)
                            .map(|point| (point, is_committed_pointer_point(&info.pointerInfo)))
                    })
                    .collect();
            }
        }

        count = 0;
        if GetPointerFrameInfo(pointer_id, &mut count, None).is_ok() && count > 0 {
            let mut infos = vec![POINTER_INFO::default(); count as usize];
            if GetPointerFrameInfo(pointer_id, &mut count, Some(infos.as_mut_ptr())).is_ok() {
                return infos[..count as usize]
                    .iter()
                    .filter_map(|info| {
                        pointer_info_to_point(hwnd, info)
                            .map(|point| (point, is_committed_pointer_point(info)))
                    })
                    .collect();
            }
        }

        Vec::new()
    }
}

fn pointer_history_points(hwnd: HWND, pointer_id: u32) -> Vec<(InkPoint, bool)> {
    unsafe {
        let mut count = 0u32;
        if GetPointerInfoHistory(pointer_id, &mut count, None).is_err() || count == 0 {
            return Vec::new();
        }

        let mut infos = vec![POINTER_INFO::default(); count as usize];
        if GetPointerInfoHistory(pointer_id, &mut count, Some(infos.as_mut_ptr())).is_err() {
            return Vec::new();
        }

        let mut points = Vec::with_capacity(count as usize);
        for info in infos[..count as usize].iter().rev() {
            if let Some(point) = pointer_info_to_point(hwnd, info) {
                points.push((point, is_committed_pointer_point(info)));
            }
        }
        points
    }
}

fn pointer_update_points(hwnd: HWND, pointer_id: u32) -> (Vec<InkPoint>, Vec<InkPoint>) {
    let history = pointer_history_points(hwnd, pointer_id);
    if !history.is_empty() {
        let mut committed = Vec::new();
        let mut preview = Vec::new();
        for (point, committed_point) in history {
            if committed_point {
                committed.push(point);
            } else {
                preview.push(point);
            }
        }
        return (committed, preview);
    }

    let frame = pointer_frame_points(hwnd, pointer_id);
    let mut committed = Vec::new();
    let mut preview = Vec::new();
    for (point, committed_point) in frame {
        if committed_point {
            committed.push(point);
        } else {
            preview.push(point);
        }
    }
    (committed, preview)
}

fn begin_pointer_stroke(hwnd: HWND, state: &RefCell<OverlayState>, pointer_id: u32) -> bool {
    let mut points = pointer_frame_points(hwnd, pointer_id);
    if points.is_empty() {
        let mut info = POINTER_INFO::default();
        if unsafe { GetPointerInfo(pointer_id, &mut info) }.is_ok()
            && let Some(point) = pointer_info_to_point(hwnd, &info)
        {
            points.push((point, is_committed_pointer_point(&info)));
        }
    }
    if points.is_empty() {
        return false;
    }

    let mut s = state.borrow_mut();
    if s.active_pointer.is_some() {
        return false;
    }

    s.active_pointer = Some(pointer_id);
    s.current_stroke.clear();
    s.preview_stroke.clear();

    let mut committed = Vec::new();
    let mut preview = Vec::new();
    for (point, committed_point) in points {
        if committed_point {
            committed.push(point);
        } else {
            preview.push(point);
        }
    }
    append_points(&mut s.current_stroke, &committed);
    append_points(&mut s.preview_stroke, &preview);
    touch_activity(&mut s);
    true
}

fn extend_pointer_stroke(hwnd: HWND, state: &RefCell<OverlayState>, pointer_id: u32) -> bool {
    let (committed, preview) = pointer_update_points(hwnd, pointer_id);
    if committed.is_empty() && preview.is_empty() {
        return false;
    }

    let mut s = state.borrow_mut();
    if s.active_pointer != Some(pointer_id) {
        return false;
    }

    append_points(&mut s.current_stroke, &committed);
    s.preview_stroke.clear();
    append_points(&mut s.preview_stroke, &preview);
    touch_activity(&mut s);
    true
}

fn finish_pointer_stroke(hwnd: HWND, state: &RefCell<OverlayState>, pointer_id: u32) -> bool {
    if state.borrow().active_pointer != Some(pointer_id) {
        return false;
    }

    let (committed, preview) = pointer_update_points(hwnd, pointer_id);
    let mut s = state.borrow_mut();
    append_points(&mut s.current_stroke, &committed);
    if let Some(last) = preview.last()
        && s.current_stroke
            .last()
            .is_none_or(|p| points_differ(p, last))
    {
        s.current_stroke.push(*last);
    }
    commit_current_stroke(&mut s);
    s.active_pointer = None;
    touch_activity(&mut s);
    true
}

fn invalidate_stroke(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn try_idle_submit(hwnd: HWND, state: &RefCell<OverlayState>) {
    let should_submit = {
        let s = state.borrow();
        if s.active_pointer.is_some() || s.action.is_some() {
            return;
        }
        let Some(last) = s.last_activity else {
            return;
        };
        last.elapsed() >= IDLE_SUBMIT && (!s.strokes.is_empty() || s.current_stroke.len() >= 2)
    };
    if should_submit {
        finish_session(hwnd, state, build_submit_action(state));
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "system" fn window_proc(
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
    let state = &*(state_ptr as *const RefCell<OverlayState>);

    match msg {
        WM_SETCURSOR => {
            if (lparam.0 as u32 & 0xFFFF) == HTCLIENT {
                let _ = SetCursor(LoadCursorW(None, IDC_ARROW).ok());
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_POINTERDOWN => {
            let pointer_id = (wparam.0 & 0xFFFF) as u32;
            if begin_pointer_stroke(hwnd, state, pointer_id) {
                invalidate_stroke(hwnd);
            }
            LRESULT(0)
        }
        WM_POINTERUPDATE => {
            let pointer_id = (wparam.0 & 0xFFFF) as u32;
            if extend_pointer_stroke(hwnd, state, pointer_id) {
                invalidate_stroke(hwnd);
            }
            LRESULT(0)
        }
        WM_POINTERUP => {
            let pointer_id = (wparam.0 & 0xFFFF) as u32;
            if finish_pointer_stroke(hwnd, state, pointer_id) {
                invalidate_stroke(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_IDLE_CHECK => {
            try_idle_submit(hwnd, state);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                finish_session(hwnd, state, OverlayAction::Cancel);
            } else if wparam.0 == VK_RETURN.0 as usize {
                finish_session(hwnd, state, build_submit_action(state));
            }
            LRESULT(0)
        }
        WM_PAINT => {
            paint_overlay(hwnd, state);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), TIMER_IDLE_CHECK);
            if state.borrow().action.is_none() {
                let action = build_submit_action(state);
                let action = if matches!(action, OverlayAction::Submit { ref strokes } if strokes.is_empty())
                {
                    OverlayAction::Cancel
                } else {
                    action
                };
                state.borrow_mut().action = Some(action);
            }
            LRESULT(0)
        }
        WM_OVERLAY_WAKE => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn paint_overlay(hwnd: HWND, state: &RefCell<OverlayState>) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        if hdc.0.is_null() {
            return;
        }

        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);

        let s = state.borrow();
        let bg_brush = CreateSolidBrush(s.config.background_color);
        let _ = FillRect(hdc, &rect, HBRUSH(bg_brush.0));
        let _ = DeleteObject(HGDIOBJ(bg_brush.0));

        let pen = CreatePen(PS_SOLID, s.config.ink_width, s.config.ink_color);
        let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));

        let live_stroke: Vec<InkPoint> = s
            .current_stroke
            .iter()
            .chain(s.preview_stroke.iter())
            .copied()
            .collect();

        for stroke in s.strokes.iter().chain(std::iter::once(&live_stroke)) {
            if stroke.is_empty() {
                continue;
            }
            if stroke.len() == 1 {
                let p = &stroke[0];
                let x = p.x as i32;
                let y = p.y as i32;
                let _ = MoveToEx(hdc, x, y, None);
                let _ = PolylineTo(hdc, &[POINT { x: x + 1, y }]);
                continue;
            }
            let first = &stroke[0];
            let _ = MoveToEx(hdc, first.x as i32, first.y as i32, None);
            for point in &stroke[1..] {
                let pt = POINT {
                    x: point.x as i32,
                    y: point.y as i32,
                };
                let _ = PolylineTo(hdc, &[pt]);
            }
        }

        SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ(pen.0));

        let _ = EndPaint(hwnd, &ps);
    }
}
