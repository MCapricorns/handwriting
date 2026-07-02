#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::upper_case_acronyms)]

use windows::Win32::Foundation::SIZE;
use windows::core::Interface;

windows_core::imp::define_interface!(
    ITfHandwriting,
    ItfHandwritingVtbl,
    0x59714133_8e20_5101_b1ae_d2cd9bad8ce5
);
impl ITfHandwriting {
    pub unsafe fn GetHandwritingState(
        &self,
        state: *mut TfHandwritingState,
    ) -> windows::core::Result<()> {
        (Interface::vtable(self).GetHandwritingState)(Interface::as_raw(self), state).ok()
    }

    pub unsafe fn SetHandwritingState(
        &self,
        state: TfHandwritingState,
    ) -> windows::core::Result<()> {
        (Interface::vtable(self).SetHandwritingState)(Interface::as_raw(self), state).ok()
    }

    pub unsafe fn RequestHandwritingForPointer(
        &self,
        pointer_id: u32,
        handwriting_stroke_id: u64,
        request_accepted: *mut windows_core::BOOL,
    ) -> windows::core::Result<()> {
        (Interface::vtable(self).RequestHandwritingForPointer)(
            Interface::as_raw(self),
            pointer_id,
            handwriting_stroke_id,
            request_accepted,
            std::ptr::null_mut(),
        )
        .ok()
    }
}

#[repr(C)]
pub struct ItfHandwritingVtbl {
    pub base: windows::core::IUnknown_Vtbl,
    pub GetHandwritingState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut TfHandwritingState,
    ) -> windows::core::HRESULT,
    pub SetHandwritingState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        TfHandwritingState,
    ) -> windows::core::HRESULT,
    pub RequestHandwritingForPointer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        u64,
        *mut windows_core::BOOL,
        *mut *mut core::ffi::c_void,
    ) -> windows::core::HRESULT,
    pub GetHandwritingDistanceThreshold:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut SIZE) -> windows::core::HRESULT,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TfHandwritingState(pub i32);
impl TfHandwritingState {
    pub const AUTO: Self = Self(0);
    pub const DISABLED: Self = Self(1);
    pub const POINTER_DELIVERY: Self = Self(3);
}
