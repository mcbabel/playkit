use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(sha256(&data))
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    let tmp = path.with_extension("playkit-tmp");
    std::fs::write(&tmp, data).map_err(|e| Error::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

pub fn copy_into(src: &Path, dst: &Path) -> Result<()> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    std::fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| Error::io(dst, e))
}

pub fn remove_empty_parents(path: &Path, stop: &Path) {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == stop || std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}
