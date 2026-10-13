use crate::{Error, Result};
use aes::cipher::KeyInit;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub struct Pak {
    reader: repak::PakReader,
    file: BufReader<File>,
}

impl Pak {
    pub fn open(path: &Path, key_hex: &str) -> Result<Pak> {
        let key = parse_key(key_hex)?;
        let mut file = BufReader::new(File::open(path).map_err(|e| Error::io(path, e))?);
        let reader = repak::PakBuilder::new()
            .key(key)
            .reader(&mut file)
            .map_err(|e| Error::Pak(e.to_string()))?;
        Ok(Pak { reader, file })
    }

    pub fn read(&mut self, game_path: &str) -> Result<Vec<u8>> {
        let inner = self.inner_path(game_path);
        self.reader
            .get(&inner, &mut self.file)
            .map_err(|e| Error::Pak(format!("{game_path}: {e}")))
    }

    pub fn contains(&self, game_path: &str) -> bool {
        let inner = self.inner_path(game_path);
        self.reader.files().contains(&inner)
    }

    fn inner_path(&self, game_path: &str) -> String {
        let mount = self.reader.mount_point().trim_start_matches("../../../");
        game_path
            .strip_prefix(mount)
            .unwrap_or(game_path)
            .to_string()
    }
}

fn parse_key(hex_key: &str) -> Result<aes::Aes256> {
    let raw = hex::decode(hex_key.trim().trim_start_matches("0x"))
        .map_err(|e| Error::Pak(format!("key: {e}")))?;
    aes::Aes256::new_from_slice(&raw).map_err(|_| Error::Pak("key must be 32 bytes".into()))
}

pub fn apply_patch(base: &[u8], patch: &[u8], name: &str) -> Result<Vec<u8>> {
    let patcher = qbsdiff::Bspatch::new(patch).map_err(|_| Error::Patch(name.into()))?;
    let mut out = Vec::with_capacity(patcher.hint_target_size() as usize);
    patcher
        .apply(base, std::io::Cursor::new(&mut out))
        .map_err(|_| Error::Patch(name.into()))?;
    Ok(out)
}

pub fn make_patch(base: &[u8], target: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    qbsdiff::Bsdiff::new(base, target)
        .compare(std::io::Cursor::new(&mut out))
        .expect("in-memory diff");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_roundtrip() {
        let base = b"Kicking_for_Inactivity... 10".repeat(50);
        let mut target = base.clone();
        target[5] = b'X';
        target.extend_from_slice(b"new tail");
        let p = make_patch(&base, &target);
        assert_eq!(apply_patch(&base, &p, "t").unwrap(), target);
    }
}
