use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

pub struct Section {
    pub name: String,
    pub start: usize,
    pub size: usize,
}

pub fn base() -> usize {
    unsafe { GetModuleHandleW(std::ptr::null()) as usize }
}

pub fn image() -> Option<&'static [u8]> {
    let b = base();
    if b == 0 {
        return None;
    }
    unsafe {
        let pe = b + *((b + 0x3c) as *const u32) as usize;
        let size = *((pe + 0x18 + 0x38) as *const u32) as usize;
        Some(std::slice::from_raw_parts(b as *const u8, size))
    }
}

pub fn sections() -> Vec<Section> {
    let b = base();
    let mut out = Vec::new();
    if b == 0 {
        return out;
    }
    unsafe {
        let pe = b + *((b + 0x3c) as *const u32) as usize;
        let n = *((pe + 6) as *const u16) as usize;
        let opt = *((pe + 20) as *const u16) as usize;
        for i in 0..n {
            let s = pe + 24 + opt + i * 40;
            let raw = std::slice::from_raw_parts(s as *const u8, 8);
            let name = String::from_utf8_lossy(raw)
                .trim_end_matches('\0')
                .to_string();
            let size = *((s + 8) as *const u32) as usize;
            let va = *((s + 12) as *const u32) as usize;
            out.push(Section {
                name,
                start: b + va,
                size,
            });
        }
    }
    out
}

pub fn section(name: &str) -> Option<&'static [u8]> {
    sections()
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| unsafe { std::slice::from_raw_parts(s.start as *const u8, s.size) })
}

pub fn parse(pattern: &str) -> Vec<Option<u8>> {
    pattern
        .split_whitespace()
        .map(|b| {
            if b == "??" || b == "?" {
                None
            } else {
                u8::from_str_radix(b, 16).ok()
            }
        })
        .collect()
}

pub fn find_in(hay: &[u8], pattern: &[Option<u8>]) -> Option<usize> {
    let first = (*pattern.first()?)?;
    let last = hay.len().checked_sub(pattern.len())?;
    (0..=last).find(|&i| {
        hay[i] == first
            && pattern
                .iter()
                .enumerate()
                .all(|(j, p)| p.is_none_or(|b| hay[i + j] == b))
    })
}

/// Absolute address of the first match of `pattern` in the executable.
pub fn find(pattern: &str) -> Option<usize> {
    let img = image()?;
    find_in(img, &parse(pattern)).map(|o| img.as_ptr() as usize + o)
}
