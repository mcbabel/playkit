use std::collections::HashSet;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;

pub struct Loose {
    project: String,
    files: HashSet<String>,
}

static LOOSE: OnceLock<Loose> = OnceLock::new();

pub fn exe_path() -> Option<PathBuf> {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) }
        as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])))
}

pub fn exe_dir() -> Option<PathBuf> {
    exe_path()?.parent().map(Path::to_path_buf)
}

fn scan(dir: &Path, root: &Path, out: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if !p
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Paks"))
            {
                scan(&p, root, out);
            }
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.insert(rel.to_string_lossy().replace('\\', "/").to_lowercase());
        }
    }
}

pub fn get() -> &'static Loose {
    LOOSE.get_or_init(|| {
        let project_dir = exe_dir().and_then(|d| d.parent()?.parent().map(Path::to_path_buf));
        let mut files = HashSet::new();
        let mut project = String::new();
        if let Some(pd) = project_dir {
            project = pd
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if let Some(root) = pd.parent() {
                scan(&pd.join("Content"), root, &mut files);
            }
        }
        crate::log::write(&format!("{} loose files in {project}/Content", files.len()));
        Loose { project, files }
    })
}

impl Loose {
    pub fn overrides(&self, wide: &[u16]) -> bool {
        if self.files.is_empty() {
            return false;
        }
        let s = String::from_utf16_lossy(wide)
            .replace('\\', "/")
            .to_lowercase();
        let marker = format!("{}/", self.project);
        let key = match s.find(&marker) {
            Some(i) => &s[i..],
            None => return false,
        };
        self.files.contains(key)
    }
}
