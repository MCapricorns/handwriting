use super::ffi::get_handwriting_stroke_id_for_pointer;
use super::interfaces::{ITfHandwriting, TfHandwritingState};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::TextServices::{CLSID_TF_ThreadMgr, ITfThreadMgr};
use windows::core::Interface;

pub struct HandwritingManager {
    thread_mgr: ITfThreadMgr,
    handwriting: ITfHandwriting,
    saved_state: TfHandwritingState,
    activated: bool,
}

impl HandwritingManager {
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let thread_mgr: ITfThreadMgr =
                CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)?;
            let activated = thread_mgr.Activate().is_ok();
            let handwriting: ITfHandwriting = thread_mgr.cast()?;

            let mut saved_state = TfHandwritingState::AUTO;
            handwriting.GetHandwritingState(&mut saved_state)?;

            Ok(Self {
                thread_mgr,
                handwriting,
                saved_state,
                activated,
            })
        }
    }

    pub fn current_state(&self) -> windows::core::Result<TfHandwritingState> {
        let mut state = TfHandwritingState::AUTO;
        unsafe {
            self.handwriting.GetHandwritingState(&mut state)?;
        }
        Ok(state)
    }

    pub fn enable_pointer_delivery(&mut self) -> windows::core::Result<()> {
        unsafe {
            self.handwriting
                .SetHandwritingState(TfHandwritingState::POINTER_DELIVERY)?;
        }
        self.saved_state = TfHandwritingState::POINTER_DELIVERY;
        Ok(())
    }

    pub fn request_handwriting_for_pointer(&self, pointer_id: u32) -> windows::core::Result<bool> {
        unsafe {
            let stroke_id = get_handwriting_stroke_id_for_pointer(pointer_id)?;
            let mut accepted = windows_core::BOOL::default();
            self.handwriting
                .RequestHandwritingForPointer(pointer_id, stroke_id, &mut accepted)?;
            Ok(accepted.as_bool())
        }
    }

    pub fn suspend_system_handwriting(&self) -> windows::core::Result<()> {
        unsafe {
            self.handwriting
                .SetHandwritingState(TfHandwritingState::DISABLED)
        }
    }

    pub fn restore_state(&self) -> windows::core::Result<()> {
        unsafe { self.handwriting.SetHandwritingState(self.saved_state) }
    }

    #[cfg(test)]
    pub fn set_state_for_test(&self, state: TfHandwritingState) -> windows::core::Result<()> {
        unsafe { self.handwriting.SetHandwritingState(state) }
    }
}

impl Drop for HandwritingManager {
    fn drop(&mut self) {
        let _ = self.restore_state();
        if self.activated {
            let _ = unsafe { self.thread_mgr.Deactivate() };
        }
    }
}
