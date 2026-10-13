//! Hooks on Blueprint functions. Every script function, whether called from native code,
//! from another Blueprint or from its own class, runs through ProcessLocalScriptFunction with
//! its parameters already evaluated in the frame's locals.

use crate::ue;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};

pub const PLSF_PATTERN: &str = "48 89 5C 24 08 48 89 74 24 20 57 48 83 EC 70 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 44 24 60 48 8B 42 20 48 8D 35 ?? ?? ?? ?? 49 8B F8 48 8B DA 80 38 04";

/// pre: return non-zero to skip the original function. post: return value ignored.
pub type HookFn = unsafe extern "C" fn(
    user: *mut c_void,
    object: *mut c_void,
    function: *mut c_void,
    params: *mut c_void,
    result: *mut c_void,
) -> i32;

#[derive(Clone, Copy)]
pub struct Hook {
    pub id: u32,
    pub pre: Option<HookFn>,
    pub post: Option<HookFn>,
    pub user: usize,
}

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static ANY: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static HOOKS: RwLock<Option<HashMap<usize, Vec<Hook>>>> = RwLock::new(None);
static PENDING: Mutex<Vec<(String, Hook)>> = Mutex::new(Vec::new());
static LAST_TRY: AtomicU64 = AtomicU64::new(0);
static LAST_COUNT: AtomicUsize = AtomicUsize::new(0);

type Native = unsafe extern "system" fn(usize, usize, usize);

unsafe extern "system" fn plsf_hook(context: usize, stack: usize, result: usize) {
    crate::mods::on_script();
    let original: Native = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Relaxed)) };
    if !ANY.load(Ordering::Relaxed) {
        return unsafe { original(context, stack, result) };
    }
    let func: usize = unsafe { *((stack + ue::FFRAME_NODE) as *const usize) };
    let hooks: Vec<Hook> = match HOOKS.read() {
        Ok(g) => match g.as_ref().and_then(|m| m.get(&func)) {
            Some(v) => v.clone(),
            None => return unsafe { original(context, stack, result) },
        },
        Err(_) => return unsafe { original(context, stack, result) },
    };
    let params: usize = unsafe { *((stack + ue::FFRAME_LOCALS) as *const usize) };
    let mut skip = false;
    for h in &hooks {
        if let Some(pre) = h.pre {
            let r = unsafe {
                pre(
                    h.user as *mut c_void,
                    context as *mut c_void,
                    func as *mut c_void,
                    params as *mut c_void,
                    result as *mut c_void,
                )
            };
            skip |= r != 0;
        }
    }
    if !skip {
        unsafe { original(context, stack, result) };
    }
    for h in &hooks {
        if let Some(post) = h.post {
            unsafe {
                post(
                    h.user as *mut c_void,
                    context as *mut c_void,
                    func as *mut c_void,
                    params as *mut c_void,
                    result as *mut c_void,
                )
            };
        }
    }
}

pub fn install() {
    let Some(target) = crate::scan::find(PLSF_PATTERN) else {
        crate::log::write("ProcessLocalScriptFunction not found: script hooks disabled");
        return;
    };
    match unsafe { crate::detour::install(target, plsf_hook as *const () as usize) } {
        Ok(tramp) => {
            ORIGINAL.store(tramp, Ordering::Relaxed);
            crate::log::write(&format!(
                "ProcessLocalScriptFunction at +{:#x}: hooked",
                target - crate::scan::base()
            ));
        }
        Err(e) => crate::log::write(&format!("ProcessLocalScriptFunction: {e}")),
    }
}

pub fn active() -> bool {
    ORIGINAL.load(Ordering::Relaxed) != 0
}

fn attach(function: usize, hook: Hook) {
    if let Ok(mut g) = HOOKS.write() {
        g.get_or_insert_with(HashMap::new)
            .entry(function)
            .or_default()
            .push(hook);
        ANY.store(true, Ordering::Relaxed);
    }
}

pub fn add(function: usize, pre: Option<HookFn>, post: Option<HookFn>, user: usize) -> u32 {
    if function == 0 || !active() {
        return 0;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) as u32;
    attach(
        function,
        Hook {
            id,
            pre,
            post,
            user,
        },
    );
    id
}

/// Hooks a function by path. If it is not loaded yet, the hook is attached once it is.
pub fn add_path(path: &str, pre: Option<HookFn>, post: Option<HookFn>, user: usize) -> u32 {
    if !active() {
        return 0;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) as u32;
    let hook = Hook {
        id,
        pre,
        post,
        user,
    };
    match crate::ue::get().map_or(0, |e| e.find(path)) {
        0 => {
            if let Ok(mut p) = PENDING.lock() {
                p.push((path.to_string(), hook));
            }
            crate::log::write(&format!("hook {path}: waiting until it is loaded"));
        }
        f => attach(f, hook),
    }
    id
}

/// Tries to attach pending hooks; cheap when nothing is pending or nothing new was loaded.
pub fn resolve_pending() {
    let Ok(mut pending) = PENDING.try_lock() else {
        return;
    };
    if pending.is_empty() {
        return;
    }
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    if now.saturating_sub(LAST_TRY.load(Ordering::Relaxed)) < 500 {
        return;
    }
    LAST_TRY.store(now, Ordering::Relaxed);
    let Some(e) = crate::ue::get() else { return };
    let count = e.num_objects();
    if count == LAST_COUNT.swap(count, Ordering::Relaxed) {
        return;
    }
    pending.retain(|(path, hook)| match e.find(path) {
        0 => true,
        f => {
            attach(f, *hook);
            crate::log::write(&format!("hook {path}: attached"));
            false
        }
    });
}

pub fn remove(id: u32) {
    if let Ok(mut g) = HOOKS.write() {
        if let Some(m) = g.as_mut() {
            for v in m.values_mut() {
                v.retain(|h| h.id != id);
            }
            m.retain(|_, v| !v.is_empty());
            ANY.store(!m.is_empty(), Ordering::Relaxed);
        }
    }
    if let Ok(mut p) = PENDING.lock() {
        p.retain(|(_, h)| h.id != id);
    }
}
