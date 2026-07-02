use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush, DEFAULT_PITCH,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, FONT_CHARSET, FillRect, HBRUSH,
    HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::core::w;

pub const ACCENT: COLORREF = COLORREF(0x00D47800);
pub const TEXT_ON_ACCENT: COLORREF = COLORREF(0x00FFFFFF);
pub const FONT_WEIGHT_SEMIBOLD: i32 = 600;

pub fn ui_font(height: i32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            FONT_CHARSET(134),
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Microsoft YaHei UI"),
        )
    }
}

pub fn delete_font(font: HFONT) {
    unsafe {
        let _ = DeleteObject(HGDIOBJ(font.0));
    }
}

pub fn fill_rect(hdc: HDC, rect: &RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let _ = FillRect(hdc, rect, HBRUSH(brush.0));
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

pub fn draw_text(hdc: HDC, rect: &RECT, text: &[u16], color: COLORREF) {
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, color);
        let mut draw_rect = *rect;
        let mut buf = text.to_vec();
        let _ = DrawTextW(
            hdc,
            &mut buf,
            &mut draw_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
}
