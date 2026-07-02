use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::core::{HRESULT, w};

type GetHandwritingStrokeIdForPointerFn = unsafe extern "system" fn(u32, *mut u64) -> HRESULT;

pub fn get_handwriting_stroke_id_for_pointer(pointer_id: u32) -> windows::core::Result<u64> {
    unsafe {
        let module = GetModuleHandleW(w!("msctf.dll"))?;
        let symbol = GetProcAddress(
            module,
            windows::core::s!("GetHandwritingStrokeIdForPointer"),
        )
        .ok_or_else(windows::core::Error::from_win32)?;
        let func: GetHandwritingStrokeIdForPointerFn = std::mem::transmute(symbol);
        let mut stroke_id = 0u64;
        func(pointer_id, &mut stroke_id).ok()?;
        Ok(stroke_id)
    }
}
