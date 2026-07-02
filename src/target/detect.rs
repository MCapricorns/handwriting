use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationValuePattern,
    TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_IsKeyboardFocusablePropertyId, UIA_TextControlTypeId,
    UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};

pub const MIN_FIELD_WIDTH: i32 = 40;
pub const MAX_FIELD_WIDTH: i32 = 900;
pub const MIN_FIELD_HEIGHT: i32 = 18;
pub const MAX_FIELD_HEIGHT: i32 = 120;

#[derive(Clone, Debug)]
pub struct TextTarget {
    pub hwnd: HWND,
    pub rect: RECT,
}

impl TextTarget {
    pub fn is_reasonable_bounds(&self) -> bool {
        let w = self.rect.right - self.rect.left;
        let h = self.rect.bottom - self.rect.top;
        (MIN_FIELD_WIDTH..=MAX_FIELD_WIDTH).contains(&w)
            && (MIN_FIELD_HEIGHT..=MAX_FIELD_HEIGHT).contains(&h)
    }
}

pub fn detect_text_target() -> Option<TextTarget> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .ok()?;

        if let Some(target) = target_from_focused(&automation)
            && is_reasonable_target(&target)
        {
            return Some(target);
        }

        if let Some(target) = target_from_cursor(&automation) {
            return Some(target);
        }

        let foreground = GetForegroundWindow();
        target_from_window(&automation, foreground)
    }
}

fn target_from_focused(automation: &IUIAutomation) -> Option<TextTarget> {
    unsafe {
        let element = automation.GetFocusedElement().ok()?;
        element_to_target(&element)
    }
}

fn target_from_cursor(automation: &IUIAutomation) -> Option<TextTarget> {
    unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point).ok()?;
        let element = automation.ElementFromPoint(point).ok()?;
        element_to_target(&element).or_else(|| walk_ancestors_for_text(automation, &element))
    }
}

fn target_from_window(automation: &IUIAutomation, root: HWND) -> Option<TextTarget> {
    if root.0.is_null() {
        return None;
    }

    unsafe {
        let element = automation.ElementFromHandle(root).ok()?;
        for control_type in [
            UIA_EditControlTypeId,
            UIA_DocumentControlTypeId,
            UIA_TextControlTypeId,
        ] {
            let condition = automation
                .CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(control_type.0))
                .ok()?;
            let focusable = automation
                .CreatePropertyCondition(UIA_IsKeyboardFocusablePropertyId, &VARIANT::from(true))
                .ok()?;
            let combined = automation.CreateAndCondition(&condition, &focusable).ok()?;
            if let Ok(found) = element.FindFirst(TreeScope_Descendants, &combined)
                && let Some(target) = element_to_target(&found)
            {
                return Some(target);
            }
        }
    }

    None
}

fn walk_ancestors_for_text(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
) -> Option<TextTarget> {
    let walker = unsafe { automation.RawViewWalker().ok()? };
    let mut current = element.clone();
    for _ in 0..12 {
        if let Some(target) = element_to_target(&current) {
            return Some(target);
        }
        current = unsafe { walker.GetParentElement(&current).ok()? };
    }
    None
}

fn is_reasonable_target(target: &TextTarget) -> bool {
    target.is_reasonable_bounds()
}

fn element_to_target(element: &IUIAutomationElement) -> Option<TextTarget> {
    unsafe {
        if !is_text_input_element(element) {
            return None;
        }

        let mut hwnd = element.CurrentNativeWindowHandle().ok().unwrap_or_default();
        if hwnd.0.is_null() {
            hwnd = GetForegroundWindow();
        }
        if hwnd.0.is_null() {
            return None;
        }

        let rect = element.CurrentBoundingRectangle().ok()?;
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return None;
        }

        Some(TextTarget { hwnd, rect })
    }
}

fn is_text_input_element(element: &IUIAutomationElement) -> bool {
    unsafe {
        let control_type = match element.CurrentControlType() {
            Ok(value) => value,
            Err(_) => return false,
        };

        if control_type == UIA_EditControlTypeId
            || control_type == UIA_DocumentControlTypeId
            || control_type == UIA_TextControlTypeId
        {
            return true;
        }

        element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .and_then(|pattern| pattern.CurrentIsReadOnly())
            .map(|read_only| !read_only.as_bool())
            .unwrap_or(false)
    }
}
