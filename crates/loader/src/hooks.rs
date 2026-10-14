use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

const IS_NON_PAK_FILENAME_ALLOWED: &str =
    "48 89 5C 24 10 48 89 6C 24 20 56 57 41 56 48 83 EC 30 48 8B F1 45 33 C0 48 8D 4C 24 20 4C 8B F2 E8 ?? ?? ?? ?? 48 8D 2D ?? ?? ?? ?? 83 78 08 00";

const PAK_FILE_FIND: &str =
    "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 41 56 41 57 48 83 EC 30 80 B9 ?? ?? ?? ?? 00 4D 8B F8 48 8B FA 48 8B D9 0F 84";

#[repr(C)]
struct FString {
    data: *const u16,
    num: i32,
    max: i32,
}

type FindFn = unsafe extern "system" fn(*mut c_void, *const FString, *mut c_void, usize) -> usize;

static ORIGINAL_FIND: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn find_hook(
    pak: *mut c_void,
    name: *const FString,
    out: *mut c_void,
    extra: usize,
) -> usize {
    if !name.is_null() {
        let s = unsafe { &*name };
        if !s.data.is_null() && s.num > 1 {
            let wide = unsafe { std::slice::from_raw_parts(s.data, (s.num - 1) as usize) };
            if crate::loose::get().overrides(wide) {
                return 0;
            }
        }
    }
    let original: FindFn = unsafe { std::mem::transmute(ORIGINAL_FIND.load(Ordering::Relaxed)) };
    unsafe { original(pak, name, out, extra) }
}

pub fn install() {
    let base = crate::scan::base();

    match crate::scan::find(IS_NON_PAK_FILENAME_ALLOWED) {
        Some(at) => {
            let ok = unsafe { crate::detour::write_code(at, &[0xB0, 0x01, 0xC3]) };
            crate::log::write(&format!(
                "IsNonPakFilenameAllowed at +{:#x}: {}",
                at - base,
                if ok { "patched" } else { "failed" }
            ));
        }
        None => crate::log::write("IsNonPakFilenameAllowed not found"),
    }

    match crate::scan::find(PAK_FILE_FIND) {
        Some(at) => match unsafe { crate::detour::install(at, find_hook as *const () as usize) } {
            Ok(tramp) => {
                ORIGINAL_FIND.store(tramp, Ordering::Relaxed);
                crate::log::write(&format!("FPakFile::Find at +{:#x}: hooked", at - base));
            }
            Err(e) => crate::log::write(&format!("FPakFile::Find: {e}")),
        },
        None => crate::log::write("FPakFile::Find not found"),
    }
}
