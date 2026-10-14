//! Native mods: DLLs in `<Binaries>/playkit-mods/` that export `playkit_mod_init(const PkApi*)`.

use crate::script::HookFn;
use crate::ue;
use std::ffi::{c_char, c_void, CStr};
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

pub const API_VERSION: u32 = 1;

pub type ObjectFn = unsafe extern "C" fn(user: *mut c_void, object: *mut c_void) -> i32;

#[repr(C)]
pub struct PkApi {
    pub version: u32,
    pub size: u32,
    pub log: unsafe extern "C" fn(*const c_char),
    pub find_object: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    pub find_function: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pub class_of: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub is_a: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32,
    pub object_name: unsafe extern "C" fn(*mut c_void, *mut c_char, i32) -> i32,
    pub object_path: unsafe extern "C" fn(*mut c_void, *mut c_char, i32) -> i32,
    pub for_each_object: unsafe extern "C" fn(*mut c_void, ObjectFn, *mut c_void),
    pub property_offset: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    pub struct_size: unsafe extern "C" fn(*mut c_void) -> i32,
    pub hook: unsafe extern "C" fn(*mut c_void, Option<HookFn>, Option<HookFn>, *mut c_void) -> u32,
    pub unhook: unsafe extern "C" fn(u32),
    pub process_event: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void),
    pub detour: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void,
    pub scan: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    pub image_base: unsafe extern "C" fn() -> *mut c_void,
    pub hook_path:
        unsafe extern "C" fn(*const c_char, Option<HookFn>, Option<HookFn>, *mut c_void) -> u32,
}

unsafe fn text(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
}

fn copy_out(s: &str, buf: *mut c_char, size: i32) -> i32 {
    if !buf.is_null() && size > 0 {
        let n = s.len().min(size as usize - 1);
        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), buf as *mut u8, n);
            *buf.add(n) = 0;
        }
    }
    s.len() as i32
}

unsafe extern "C" fn api_log(msg: *const c_char) {
    crate::log::write(&format!("[mod] {}", unsafe { text(msg) }));
}

unsafe extern "C" fn api_find_object(path: *const c_char) -> *mut c_void {
    ue::get().map_or(0, |e| e.find(&unsafe { text(path) })) as *mut c_void
}

unsafe extern "C" fn api_find_function(class: *mut c_void, name: *const c_char) -> *mut c_void {
    ue::get().map_or(0, |e| {
        e.find_function(class as usize, &unsafe { text(name) })
    }) as *mut c_void
}

unsafe extern "C" fn api_class_of(obj: *mut c_void) -> *mut c_void {
    if obj.is_null() {
        return std::ptr::null_mut();
    }
    ue::get().map_or(0, |e| e.class_of(obj as usize)) as *mut c_void
}

unsafe extern "C" fn api_is_a(obj: *mut c_void, class: *mut c_void) -> i32 {
    if obj.is_null() || class.is_null() {
        return 0;
    }
    ue::get().is_some_and(|e| e.is_a(obj as usize, class as usize)) as i32
}

unsafe extern "C" fn api_object_name(obj: *mut c_void, buf: *mut c_char, size: i32) -> i32 {
    let s = match (ue::get(), obj.is_null()) {
        (Some(e), false) => e.object_name(obj as usize),
        _ => String::new(),
    };
    copy_out(&s, buf, size)
}

unsafe extern "C" fn api_object_path(obj: *mut c_void, buf: *mut c_char, size: i32) -> i32 {
    let s = match (ue::get(), obj.is_null()) {
        (Some(e), false) => e.path(obj as usize),
        _ => String::new(),
    };
    copy_out(&s, buf, size)
}

unsafe extern "C" fn api_for_each_object(class: *mut c_void, f: ObjectFn, user: *mut c_void) {
    let Some(e) = ue::get() else { return };
    for o in e.objects() {
        if (class.is_null() || e.is_a(o, class as usize))
            && unsafe { f(user, o as *mut c_void) } != 0
        {
            break;
        }
    }
}

unsafe extern "C" fn api_property_offset(s: *mut c_void, name: *const c_char) -> i32 {
    let Some(e) = ue::get() else { return -1 };
    if s.is_null() {
        return -1;
    }
    match e.find_property(s as usize, &unsafe { text(name) }) {
        0 => -1,
        p => e.property_offset(p),
    }
}

unsafe extern "C" fn api_struct_size(s: *mut c_void) -> i32 {
    match (ue::get(), s.is_null()) {
        (Some(e), false) => e.struct_size(s as usize),
        _ => -1,
    }
}

unsafe extern "C" fn api_hook(
    function: *mut c_void,
    pre: Option<HookFn>,
    post: Option<HookFn>,
    user: *mut c_void,
) -> u32 {
    crate::script::add(function as usize, pre, post, user as usize)
}

unsafe extern "C" fn api_hook_path(
    path: *const c_char,
    pre: Option<HookFn>,
    post: Option<HookFn>,
    user: *mut c_void,
) -> u32 {
    crate::script::add_path(&unsafe { text(path) }, pre, post, user as usize)
}

unsafe extern "C" fn api_unhook(id: u32) {
    crate::script::remove(id)
}

unsafe extern "C" fn api_process_event(obj: *mut c_void, func: *mut c_void, params: *mut c_void) {
    if let (Some(e), false, false) = (ue::get(), obj.is_null(), func.is_null()) {
        unsafe { e.process_event(obj as usize, func as usize, params as usize) };
    }
}

unsafe extern "C" fn api_detour(target: *mut c_void, hook: *mut c_void) -> *mut c_void {
    match unsafe { crate::detour::install(target as usize, hook as usize) } {
        Ok(t) => t as *mut c_void,
        Err(e) => {
            crate::log::write(&format!("[mod] detour failed: {e}"));
            std::ptr::null_mut()
        }
    }
}

unsafe extern "C" fn api_scan(pattern: *const c_char) -> *mut c_void {
    crate::scan::find(&unsafe { text(pattern) }).unwrap_or(0) as *mut c_void
}

unsafe extern "C" fn api_image_base() -> *mut c_void {
    crate::scan::base() as *mut c_void
}

static API: PkApi = PkApi {
    version: API_VERSION,
    size: std::mem::size_of::<PkApi>() as u32,
    log: api_log,
    find_object: api_find_object,
    find_function: api_find_function,
    class_of: api_class_of,
    is_a: api_is_a,
    object_name: api_object_name,
    object_path: api_object_path,
    for_each_object: api_for_each_object,
    property_offset: api_property_offset,
    struct_size: api_struct_size,
    hook: api_hook,
    unhook: api_unhook,
    process_event: api_process_event,
    detour: api_detour,
    scan: api_scan,
    image_base: api_image_base,
    hook_path: api_hook_path,
};

static STATE: AtomicU8 = AtomicU8::new(0);
static CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_SDK_CHECK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

type InitFn = unsafe extern "C" fn(*const PkApi) -> i32;

/// Called on the game thread for every Blueprint function call.
pub fn on_script() {
    if STATE.load(Ordering::Relaxed) == 0
        && STATE
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    {
        start();
        STATE.store(2, Ordering::Release);
    }
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    if n.is_multiple_of(64) {
        crate::script::resolve_pending();
    }
    if n.is_multiple_of(64) {
        let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        if now.saturating_sub(LAST_SDK_CHECK.load(Ordering::Relaxed)) >= 2000 {
            LAST_SDK_CHECK.store(now, Ordering::Relaxed);
            crate::sdk::check_request();
        }
    }
}

fn start() {
    let Some(engine) = ue::get() else {
        crate::log::write("engine structures not found: native mods disabled");
        return;
    };
    crate::log::write(&format!("engine ready: {} objects", engine.num_objects()));
    let Some(dir) = crate::loose::exe_dir().map(|d| d.join("playkit-mods")) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut dlls: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")))
        .collect();
    dlls.sort();
    for path in dlls {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            crate::log::write(&format!("mod {name}: cannot load"));
            continue;
        }
        let Some(init) =
            (unsafe { GetProcAddress(module, c"playkit_mod_init".as_ptr() as *const u8) })
        else {
            crate::log::write(&format!("mod {name}: no playkit_mod_init export"));
            continue;
        };
        let init: InitFn = unsafe { std::mem::transmute(init) };
        let r = unsafe { init(&API) };
        crate::log::write(&format!(
            "mod {name}: loaded ({})",
            if r == 0 { "ok" } else { "init failed" }
        ));
    }
}
