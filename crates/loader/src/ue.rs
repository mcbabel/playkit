//! Unreal Engine 4.23 to 4.27 runtime access: names, objects and reflection.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const UOBJECT_CLASS: usize = 0x10;
pub const UOBJECT_NAME: usize = 0x18;
pub const UOBJECT_OUTER: usize = 0x20;
pub const USTRUCT_SUPER: usize = 0x40;
pub const USTRUCT_CHILDREN: usize = 0x48;
pub const USTRUCT_CHILD_PROPERTIES: usize = 0x50;
pub const USTRUCT_SIZE: usize = 0x58;
pub const UFIELD_NEXT: usize = 0x28;
pub const UFUNCTION_FLAGS: usize = 0xB0;
pub const UENUM_NAMES: usize = 0x40;
pub const FFIELD_CLASS: usize = 0x08;
pub const FFIELD_NEXT: usize = 0x20;
pub const FFIELD_NAME: usize = 0x28;
pub const FPROPERTY_ARRAY_DIM: usize = 0x38;
pub const FPROPERTY_ELEMENT_SIZE: usize = 0x3C;
pub const FPROPERTY_FLAGS: usize = 0x40;
pub const FPROPERTY_OFFSET: usize = 0x4C;
pub const FPROPERTY_EXTRA: usize = 0x78;
pub const FFRAME_NODE: usize = 0x10;
pub const FFRAME_LOCALS: usize = 0x28;

pub const CPF_PARM: u64 = 0x80;
pub const CPF_OUT_PARM: u64 = 0x100;
pub const CPF_RETURN_PARM: u64 = 0x400;
pub const FUNC_STATIC: u32 = 0x2000;

pub struct Engine {
    name_blocks: usize,
    objects: usize,
    pub process_event_index: usize,
    paths: Mutex<HashMap<String, usize>>,
}

static ENGINE: OnceLock<Option<Engine>> = OnceLock::new();

unsafe fn rd<T: Copy>(addr: usize) -> T {
    unsafe { std::ptr::read_unaligned(addr as *const T) }
}

pub fn get() -> Option<&'static Engine> {
    ENGINE.get_or_init(Engine::discover).as_ref()
}

impl Engine {
    fn discover() -> Option<Engine> {
        let base = crate::scan::base();
        let data = crate::scan::section(".data")?;
        let objects = find_object_array(data)?;
        let name_blocks = find_name_pool(data)?;
        crate::log::write(&format!(
            "engine: GUObjectArray at +{:#x}, FNamePool blocks at +{:#x}",
            objects - base,
            name_blocks - base
        ));
        Some(Engine {
            name_blocks,
            objects,
            process_event_index: 68,
            paths: Mutex::new(HashMap::new()),
        })
    }

    pub fn name(&self, index: u32) -> String {
        unsafe {
            let block: usize = rd(self.name_blocks + 8 * (index >> 16) as usize);
            if block == 0 {
                return String::new();
            }
            let entry = block + 2 * (index & 0xFFFF) as usize;
            let header: u16 = rd(entry);
            let len = (header >> 6) as usize;
            if header & 1 != 0 {
                let w = std::slice::from_raw_parts((entry + 2) as *const u16, len);
                String::from_utf16_lossy(w)
            } else {
                let b = std::slice::from_raw_parts((entry + 2) as *const u8, len);
                b.iter().map(|&c| c as char).collect()
            }
        }
    }

    pub fn fname(&self, at: usize) -> String {
        let (index, number): (u32, u32) = unsafe { (rd(at), rd(at + 4)) };
        let s = self.name(index);
        if number == 0 {
            s
        } else {
            format!("{s}_{}", number - 1)
        }
    }

    pub fn num_objects(&self) -> usize {
        unsafe { rd::<i32>(self.objects + 0x10 + 0x14) as usize }
    }

    pub fn object(&self, i: usize) -> usize {
        unsafe {
            let chunks: usize = rd(self.objects + 0x10);
            let chunk: usize = rd(chunks + 8 * (i / 65536));
            if chunk == 0 {
                return 0;
            }
            rd(chunk + 0x18 * (i % 65536))
        }
    }

    pub fn objects(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.num_objects())
            .map(|i| self.object(i))
            .filter(|&o| o != 0)
    }

    pub fn class_of(&self, obj: usize) -> usize {
        unsafe { rd(obj + UOBJECT_CLASS) }
    }

    pub fn outer(&self, obj: usize) -> usize {
        unsafe { rd(obj + UOBJECT_OUTER) }
    }

    pub fn object_name(&self, obj: usize) -> String {
        self.fname(obj + UOBJECT_NAME)
    }

    /// Full path such as `/Script/Engine.Actor` or `/Game/Base/GI.GI_C:ReceiveInit`.
    pub fn path(&self, obj: usize) -> String {
        let mut parts = Vec::new();
        let mut o = obj;
        while o != 0 {
            parts.push(self.object_name(o));
            o = self.outer(o);
        }
        parts.reverse();
        let mut s = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                s.push(if i == 2 { ':' } else { '.' });
            }
            s.push_str(p);
        }
        s
    }

    pub fn super_struct(&self, s: usize) -> usize {
        unsafe { rd(s + USTRUCT_SUPER) }
    }

    pub fn is_a(&self, obj: usize, class: usize) -> bool {
        let mut c = self.class_of(obj);
        while c != 0 {
            if c == class {
                return true;
            }
            c = self.super_struct(c);
        }
        false
    }

    /// Finds an object by full path. `.` and `:` are interchangeable.
    pub fn find(&self, path: &str) -> usize {
        let key = path.replace(':', ".");
        if let Some(&o) = self
            .paths
            .lock()
            .ok()
            .and_then(|m| m.get(&key).copied())
            .as_ref()
        {
            if o != 0 && self.path(o).replace(':', ".") == key {
                return o;
            }
        }
        let leaf = key.rsplit('.').next().unwrap_or("");
        for o in self.objects() {
            if self.object_name(o) == leaf && self.path(o).replace(':', ".") == key {
                if let Ok(mut m) = self.paths.lock() {
                    m.insert(key, o);
                }
                return o;
            }
        }
        0
    }

    pub fn find_function(&self, class: usize, name: &str) -> usize {
        let mut c = class;
        while c != 0 {
            let mut f: usize = unsafe { rd(c + USTRUCT_CHILDREN) };
            while f != 0 {
                if self.object_name(f) == name {
                    return f;
                }
                f = unsafe { rd(f + UFIELD_NEXT) };
            }
            c = self.super_struct(c);
        }
        0
    }

    pub fn properties(&self, s: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut p: usize = unsafe { rd(s + USTRUCT_CHILD_PROPERTIES) };
        while p != 0 {
            out.push(p);
            p = unsafe { rd(p + FFIELD_NEXT) };
        }
        out
    }

    pub fn property_name(&self, p: usize) -> String {
        self.fname(p + FFIELD_NAME)
    }

    pub fn property_type(&self, p: usize) -> String {
        let class: usize = unsafe { rd(p + FFIELD_CLASS) };
        self.fname(class)
    }

    pub fn property_offset(&self, p: usize) -> i32 {
        unsafe { rd(p + FPROPERTY_OFFSET) }
    }

    pub fn property_size(&self, p: usize) -> i32 {
        unsafe { rd::<i32>(p + FPROPERTY_ELEMENT_SIZE) * rd::<i32>(p + FPROPERTY_ARRAY_DIM) }
    }

    pub fn property_flags(&self, p: usize) -> u64 {
        unsafe { rd(p + FPROPERTY_FLAGS) }
    }

    /// Looks a property up in a struct and its parents.
    pub fn find_property(&self, s: usize, name: &str) -> usize {
        let mut c = s;
        while c != 0 {
            for p in self.properties(c) {
                if self.property_name(p) == name {
                    return p;
                }
            }
            c = self.super_struct(c);
        }
        0
    }

    pub fn function_flags(&self, f: usize) -> u32 {
        unsafe { rd(f + UFUNCTION_FLAGS) }
    }

    pub fn struct_size(&self, s: usize) -> i32 {
        unsafe { rd(s + USTRUCT_SIZE) }
    }

    /// Calls UObject::ProcessEvent (virtual) on `obj`.
    pub unsafe fn process_event(&self, obj: usize, func: usize, params: usize) {
        unsafe {
            let vtable: usize = rd(obj);
            let pe: usize = rd(vtable + 8 * self.process_event_index);
            let f: extern "system" fn(usize, usize, usize) = std::mem::transmute(pe);
            f(obj, func, params);
        }
    }
}

fn readable(addr: usize) -> bool {
    addr > 0x10000 && addr < 0x7FFF_FFFF_FFFF
}

/// FChunkedFixedUObjectArray inside FUObjectArray: chunk table, preallocated objects,
/// MaxElements, NumElements, MaxChunks, NumChunks. Returns the FUObjectArray address.
fn find_object_array(data: &[u8]) -> Option<usize> {
    let base = data.as_ptr() as usize;
    let mut off = 0x10;
    while off + 0x28 <= data.len() {
        let at = |o: usize| i32::from_le_bytes(data[off + o..off + o + 4].try_into().unwrap());
        let (max, num, max_chunks, chunks) = (at(16), at(20), at(24), at(28));
        if num > 1000
            && num <= max
            && max <= 0x0100_0000
            && chunks == (num + 0xFFFF) / 0x10000
            && max_chunks == (max + 0xFFFF) / 0x10000
        {
            let table = u64::from_le_bytes(data[off..off + 8].try_into().unwrap()) as usize;
            if readable(table) {
                return Some(base + off - 0x10);
            }
        }
        off += 4;
    }
    None
}

/// FNamePool: its block table starts with a block whose first entries are "None" and "ByteProperty".
fn find_name_pool(data: &[u8]) -> Option<usize> {
    let base = data.as_ptr() as usize;
    let mut off = 0;
    while off + 8 <= data.len() {
        let p = u64::from_le_bytes(data[off..off + 8].try_into().unwrap()) as usize;
        if readable(p) && p & 1 == 0 && looks_like_first_block(p) {
            return Some(base + off);
        }
        off += 8;
    }
    None
}

fn looks_like_first_block(p: usize) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT};
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let n = unsafe {
        VirtualQuery(
            p as *const std::ffi::c_void,
            &mut info,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if n == 0 || info.State != MEM_COMMIT || info.Protect & 0x66 == 0 || info.Protect & 0x101 != 0 {
        return false;
    }
    let end = info.BaseAddress as usize + info.RegionSize;
    if p + 20 > end {
        return false;
    }
    let b = unsafe { std::slice::from_raw_parts(p as *const u8, 20) };
    &b[2..6] == b"None" && &b[8..20] == b"ByteProperty"
}
