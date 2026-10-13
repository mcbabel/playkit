use std::ffi::c_void;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const IS_NON_PAK_FILENAME_ALLOWED: &str =
    "48 89 5C 24 10 48 89 6C 24 20 56 57 41 56 48 83 EC 30 48 8B F1 45 33 C0 48 8D 4C 24 20 4C 8B F2 E8 ?? ?? ?? ?? 48 8D 2D ?? ?? ?? ?? 83 78 08 00";

const PAK_FILE_FIND: &str =
    "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 41 56 41 57 48 83 EC 30 80 B9 ?? ?? ?? ?? 00 4D 8B F8 48 8B FA 48 8B D9 0F 84";

const FIND_STOLEN: usize = 15;

#[repr(C)]
struct FString {
    data: *const u16,
    num: i32,
    max: i32,
}

type FindFn = unsafe extern "system" fn(*mut c_void, *const FString, *mut c_void, usize) -> usize;

static mut ORIGINAL_FIND: usize = 0;

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
    let original: FindFn = unsafe { std::mem::transmute(ORIGINAL_FIND) };
    unsafe { original(pak, name, out, extra) }
}

fn parse(pattern: &str) -> Vec<Option<u8>> {
    pattern
        .split_whitespace()
        .map(|b| {
            if b == "??" {
                None
            } else {
                u8::from_str_radix(b, 16).ok()
            }
        })
        .collect()
}

fn image() -> Option<&'static [u8]> {
    unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            return None;
        }
        let pe = base.add(*(base.add(0x3c) as *const u32) as usize);
        let size = *(pe.add(0x18 + 0x38) as *const u32) as usize;
        Some(std::slice::from_raw_parts(base, size))
    }
}

fn find(image: &[u8], pattern: &[Option<u8>]) -> Option<usize> {
    let first = pattern[0]?;
    let last = image.len().checked_sub(pattern.len())?;
    (0..=last).find(|&i| {
        image[i] == first
            && pattern
                .iter()
                .enumerate()
                .all(|(j, p)| p.is_none_or(|b| image[i + j] == b))
    })
}

unsafe fn write_code(at: *mut u8, bytes: &[u8]) -> bool {
    let mut old = 0u32;
    unsafe {
        if VirtualProtect(
            at as *const c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        ) == 0
        {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
        VirtualProtect(at as *const c_void, bytes.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), at as *const c_void, bytes.len());
    }
    true
}

fn abs_jump(target: usize) -> [u8; 14] {
    let mut j = [0xFF, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    j[6..].copy_from_slice(&(target as u64).to_le_bytes());
    j
}

pub fn install() {
    crate::log::reset();
    let Some(img) = image() else { return };
    let base = img.as_ptr() as usize;

    match find(img, &parse(IS_NON_PAK_FILENAME_ALLOWED)) {
        Some(off) => {
            let ok = unsafe { write_code((base + off) as *mut u8, &[0xB0, 0x01, 0xC3]) };
            crate::log::write(&format!(
                "IsNonPakFilenameAllowed at +{off:#x}: {}",
                if ok { "patched" } else { "failed" }
            ));
        }
        None => crate::log::write("IsNonPakFilenameAllowed not found"),
    }

    match find(img, &parse(PAK_FILE_FIND)) {
        Some(off) => {
            let target = (base + off) as *mut u8;
            let tramp = unsafe {
                VirtualAlloc(
                    std::ptr::null(),
                    64,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                ) as *mut u8
            };
            if tramp.is_null() {
                crate::log::write("FPakFile::Find: no memory for trampoline");
                return;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(target, tramp, FIND_STOLEN);
                let back = abs_jump(target as usize + FIND_STOLEN);
                std::ptr::copy_nonoverlapping(back.as_ptr(), tramp.add(FIND_STOLEN), back.len());
                ORIGINAL_FIND = tramp as usize;
            }
            let mut patch = [0x90u8; FIND_STOLEN];
            patch[..14].copy_from_slice(&abs_jump(find_hook as *const () as usize));
            let ok = unsafe { write_code(target, &patch) };
            crate::log::write(&format!(
                "FPakFile::Find at +{off:#x}: {}",
                if ok { "hooked" } else { "failed" }
            ));
        }
        None => crate::log::write("FPakFile::Find not found"),
    }
}
