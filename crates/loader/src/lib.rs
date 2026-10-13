#![cfg(windows)]

mod detour;
mod hooks;
mod log;
mod loose;
mod mods;
mod proxy;
mod scan;
mod script;
mod sdk;
mod ue;

use std::ffi::c_void;
use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(_module: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        proxy::load_real_dxgi();
        hooks::install();
        script::install();
    }
    TRUE
}
