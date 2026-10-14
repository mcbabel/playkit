//! Writes the reflection database and C++ headers. Request it by creating
//! `playkit-sdk.request` next to the game executable. Everything loaded at that moment is merged
//! into `playkit-sdk/reflection.json`, so requests in several game sessions add up, and
//! `playkit-sdk/playkit_sdk.hpp` is generated from the whole database.
//! If the request file contains `watch`, it stays and the database is updated again whenever
//! the game has loaded or unloaded types, e.g. after every map change.
//! If it contains `all`, the loader first loads every asset listed in `playkit-sdk.load` (one
//! object path per line, like `/Game/Base/GM_Escape.GM_Escape`) in batches and dumps after each
//! batch, so the database covers the whole game without playing through it. The asset being
//! loaded is noted in `playkit-sdk.progress`; if one crashes the game, the next session adds it
//! to `playkit-sdk.skip` and continues after it.

use crate::ue::{self, Engine};
use playkit_reflect::{Db, Enum, Func, Prop, Struct};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

const LOAD_BATCH: usize = 150;
struct LoadQueue {
    paths: Vec<String>,
    next: usize,
    skip: Vec<String>,
}
static LOAD_QUEUE: Mutex<Option<LoadQueue>> = Mutex::new(None);

static LAST_COUNT: AtomicUsize = AtomicUsize::new(0);
static LAST_TIME: AtomicU64 = AtomicU64::new(0);
const WATCH_INTERVAL_MS: u64 = 15_000;
const TYPE_CLASSES: &[&str] = &[
    "/Script/CoreUObject.Class",
    "/Script/CoreUObject.ScriptStruct",
    "/Script/CoreUObject.Enum",
    "/Script/Engine.BlueprintGeneratedClass",
    "/Script/UMG.WidgetBlueprintGeneratedClass",
    "/Script/Engine.AnimBlueprintGeneratedClass",
    "/Script/Engine.UserDefinedStruct",
    "/Script/Engine.UserDefinedEnum",
];

/// Number of loaded classes, structs and enums: changes when the game loads or unloads types.
fn type_count(e: &Engine) -> usize {
    let metas: Vec<usize> = TYPE_CLASSES
        .iter()
        .map(|p| e.find(p))
        .filter(|&c| c != 0)
        .collect();
    e.objects()
        .filter(|&o| metas.contains(&e.class_of(o)))
        .count()
}

pub fn check_request() {
    let Some(dir) = crate::loose::exe_dir() else {
        return;
    };
    let req = dir.join("playkit-sdk.request");
    let Ok(text) = std::fs::read_to_string(&req) else {
        return;
    };
    let Some(e) = ue::get() else { return };
    let mode = text.trim().to_ascii_lowercase();
    if mode == "all" {
        let done = load_batch(e, &dir);
        dump(e, &dir);
        if done {
            let _ = std::fs::remove_file(&req);
        }
        return;
    }
    if mode == "watch" {
        let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        if now.saturating_sub(LAST_TIME.load(Ordering::Relaxed)) < WATCH_INTERVAL_MS {
            return;
        }
        let count = type_count(e);
        if count == LAST_COUNT.load(Ordering::Relaxed) {
            return;
        }
        LAST_COUNT.store(count, Ordering::Relaxed);
        LAST_TIME.store(now, Ordering::Relaxed);
    } else {
        let _ = std::fs::remove_file(&req);
    }
    dump(e, &dir);
}

fn dump(e: &Engine, dir: &std::path::Path) {
    let out = dir.join("playkit-sdk");
    let _ = std::fs::create_dir_all(&out);
    let json = out.join("reflection.json");
    let fresh = collect(e);
    let (classes, enums) = (fresh.classes.len(), fresh.enums.len());
    let mut db = Db::load(&json).unwrap_or_default();
    let before = db.classes.len() + db.enums.len();
    db.merge(fresh);
    let added = db.classes.len() + db.enums.len() - before;
    let ok = db.save(&json).is_ok()
        && std::fs::write(out.join("playkit_sdk.hpp"), playkit_reflect::generate(&db)).is_ok();
    crate::log::write(&format!(
        "sdk: {classes} classes and structs, {enums} enums loaded, {added} new, {} in total ({})",
        db.classes.len() + db.enums.len(),
        if ok { "ok" } else { "write failed" }
    ));
}

/// Loads the next batch of `playkit-sdk.load`. Returns true when the list is done.
fn load_batch(e: &Engine, dir: &std::path::Path) -> bool {
    let Ok(mut q) = LOAD_QUEUE.lock() else {
        return true;
    };
    let progress = dir.join("playkit-sdk.progress");
    if q.is_none() {
        let read = |name: &str| -> Vec<String> {
            std::fs::read_to_string(dir.join(name))
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect()
        };
        let mut skip = read("playkit-sdk.skip");
        let paths = read("playkit-sdk.load");
        // "next N": the last session stopped cleanly before asset N.
        // "N": the last session ended while loading asset N, so skip it from now on.
        let mut start = 0;
        let state = std::fs::read_to_string(&progress).unwrap_or_default();
        if let Some(n) = state.trim().strip_prefix("next ") {
            start = n.parse().unwrap_or(0);
        } else if let Ok(n) = state.trim().parse::<usize>() {
            if let Some(p) = paths.get(n) {
                crate::log::write(&format!("sdk: {p} ended the last session, skipping it"));
                skip.push(p.clone());
                let _ = std::fs::write(dir.join("playkit-sdk.skip"), skip.join("\n") + "\n");
            }
            start = n + 1;
        }
        start = start.min(paths.len());
        crate::log::write(&format!(
            "sdk: loading {} assets from playkit-sdk.load, starting at {start}",
            paths.len()
        ));
        *q = Some(LoadQueue {
            paths,
            next: start,
            skip,
        });
    }
    let Some(LoadQueue { paths, next, skip }) = q.as_mut() else {
        return true;
    };
    let Some(loader) = AssetLoader::new(e) else {
        crate::log::write("sdk: LoadAsset_Blocking not found, cannot load assets");
        return true;
    };
    let end = (*next + LOAD_BATCH).min(paths.len());
    let mut loaded = 0;
    for (i, p) in paths.iter().enumerate().take(end).skip(*next) {
        if skip.contains(p) {
            continue;
        }
        let _ = std::fs::write(&progress, i.to_string());
        if loader.load(e, p) {
            loaded += 1;
        }
    }
    if end >= paths.len() {
        let _ = std::fs::remove_file(&progress);
    } else {
        let _ = std::fs::write(&progress, format!("next {end}"));
    }
    crate::log::write(&format!(
        "sdk: loaded {loaded} of {} ({}/{})",
        end - *next,
        end,
        paths.len()
    ));
    *next = end;
    *next >= paths.len()
}

/// Calls KismetStringLibrary.Conv_StringToName and KismetSystemLibrary.LoadAsset_Blocking.
struct AssetLoader {
    string_lib: usize,
    to_name: usize,
    to_name_in: usize,
    to_name_out: usize,
    to_name_size: usize,
    system_lib: usize,
    load: usize,
    load_asset: usize,
    load_out: usize,
    load_size: usize,
}

impl AssetLoader {
    fn new(e: &Engine) -> Option<AssetLoader> {
        let string_lib = e.find("/Script/Engine.Default__KismetStringLibrary");
        let system_lib = e.find("/Script/Engine.Default__KismetSystemLibrary");
        let to_name = e.find_function(
            e.find("/Script/Engine.KismetStringLibrary"),
            "Conv_StringToName",
        );
        let load = e.find_function(
            e.find("/Script/Engine.KismetSystemLibrary"),
            "LoadAsset_Blocking",
        );
        if [string_lib, system_lib, to_name, load].contains(&0) {
            return None;
        }
        let off = |f: usize, n: &str| match e.find_property(f, n) {
            0 => None,
            p => Some(e.property_offset(p) as usize),
        };
        Some(AssetLoader {
            string_lib,
            to_name,
            to_name_in: off(to_name, "InString")?,
            to_name_out: off(to_name, "ReturnValue")?,
            to_name_size: e.struct_size(to_name).max(0) as usize,
            system_lib,
            load,
            load_asset: off(load, "Asset")?,
            load_out: off(load, "ReturnValue")?,
            load_size: e.struct_size(load).max(0) as usize,
        })
    }

    fn load(&self, e: &Engine, path: &str) -> bool {
        let mut wide: Vec<u16> = path.encode_utf16().collect();
        wide.push(0);
        let mut buf = vec![0u8; self.to_name_size.max(32)];
        unsafe {
            let at = buf.as_mut_ptr().add(self.to_name_in);
            std::ptr::write_unaligned(at as *mut usize, wide.as_ptr() as usize);
            std::ptr::write_unaligned(at.add(8) as *mut i32, wide.len() as i32);
            std::ptr::write_unaligned(at.add(12) as *mut i32, wide.len() as i32);
            e.process_event(self.string_lib, self.to_name, buf.as_mut_ptr() as usize);
        }
        let name: [u8; 8] = buf[self.to_name_out..self.to_name_out + 8]
            .try_into()
            .unwrap_or([0; 8]);
        let mut params = vec![0u8; self.load_size.max(0x30)];
        // TSoftObjectPtr: weak pointer (8), tag (4), padding (4), then FSoftObjectPath.AssetPathName
        params[self.load_asset + 16..self.load_asset + 24].copy_from_slice(&name);
        unsafe { e.process_event(self.system_lib, self.load, params.as_mut_ptr() as usize) };
        let result = usize::from_le_bytes(
            params[self.load_out..self.load_out + 8]
                .try_into()
                .unwrap_or([0; 8]),
        );
        result != 0
    }
}

unsafe fn rd<T: Copy>(a: usize) -> T {
    unsafe { std::ptr::read_unaligned(a as *const T) }
}

fn collect(e: &Engine) -> Db {
    let class_cls = e.find("/Script/CoreUObject.Class");
    let struct_cls = e.find("/Script/CoreUObject.ScriptStruct");
    let enum_cls = e.find("/Script/CoreUObject.Enum");
    let mut db = Db::default();
    for o in e.objects() {
        if e.object_name(o).starts_with("Default__") {
            continue;
        }
        let kind = if class_cls != 0 && e.is_a(o, class_cls) {
            "class"
        } else if struct_cls != 0 && e.is_a(o, struct_cls) {
            "struct"
        } else if enum_cls != 0 && e.is_a(o, enum_cls) {
            db.enums.push(Enum {
                path: e.path(o),
                values: enum_values(e, o),
            });
            continue;
        } else {
            continue;
        };
        let sup = e.super_struct(o);
        db.classes.push(Struct {
            kind: kind.into(),
            path: e.path(o),
            cpp: String::new(),
            super_path: if sup == 0 { String::new() } else { e.path(sup) },
            size: e.struct_size(o),
            properties: props(e, o),
            functions: if kind == "class" {
                functions(o)
                    .into_iter()
                    .map(|f| Func {
                        name: e.object_name(f),
                        flags: e.function_flags(f),
                        size: e.struct_size(f),
                        params: props(e, f),
                    })
                    .collect()
            } else {
                Vec::new()
            },
        });
    }
    db
}

fn props(e: &Engine, s: usize) -> Vec<Prop> {
    e.properties(s)
        .into_iter()
        .map(|p| Prop {
            name: e.property_name(p),
            ty: type_desc(e, p),
            offset: e.property_offset(p),
            size: e.property_size(p),
            flags: e.property_flags(p),
            dim: unsafe { rd(p + ue::FPROPERTY_ARRAY_DIM) },
            mask: if e.property_type(p) == "BoolProperty" {
                unsafe { rd(p + ue::FPROPERTY_EXTRA + 3) }
            } else {
                0xFF
            },
        })
        .collect()
}

fn type_desc(e: &Engine, p: usize) -> String {
    let t = e.property_type(p);
    let x = |o: usize| unsafe { rd::<usize>(p + ue::FPROPERTY_EXTRA + o) };
    let path = |o: usize| if o == 0 { String::new() } else { e.path(o) };
    match t.as_str() {
        "StructProperty" => format!("Struct<{}>", path(x(0))),
        "ObjectProperty" | "ObjectPtrProperty" | "WeakObjectProperty" | "LazyObjectProperty"
        | "SoftObjectProperty" | "InterfaceProperty" => format!("{t}<{}>", path(x(0))),
        "ClassProperty" | "SoftClassProperty" => format!("{t}<{}>", path(x(8))),
        "ArrayProperty" | "SetProperty" => format!("{t}<{}>", type_desc(e, x(0))),
        "MapProperty" => format!("{t}<{},{}>", type_desc(e, x(0)), type_desc(e, x(8))),
        "EnumProperty" => format!("{t}<{}>", path(x(8))),
        "ByteProperty" if x(0) != 0 => format!("ByteProperty<{}>", path(x(0))),
        "DelegateProperty"
        | "MulticastInlineDelegateProperty"
        | "MulticastSparseDelegateProperty" => {
            format!("{t}<{}>", path(x(0)))
        }
        _ => t,
    }
}

fn functions(class: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut f: usize = unsafe { rd(class + ue::USTRUCT_CHILDREN) };
    while f != 0 {
        out.push(f);
        f = unsafe { rd(f + ue::UFIELD_NEXT) };
    }
    out
}

fn enum_values(e: &Engine, en: usize) -> Vec<(String, i64)> {
    let data: usize = unsafe { rd(en + ue::UENUM_NAMES) };
    let num: i32 = unsafe { rd(en + ue::UENUM_NAMES + 8) };
    (0..num.max(0) as usize)
        .map(|i| {
            let at = data + 16 * i;
            (e.fname(at), unsafe { rd::<i64>(at + 8) })
        })
        .collect()
}
