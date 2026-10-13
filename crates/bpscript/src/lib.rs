pub mod ast;
pub mod lower;
pub mod names;
pub mod parse;
pub mod patch;
pub mod print;

use ast::{Function, Stmt};
use lower::{ke, Lower, KE};
use names::Want;
use parse::Item;
use std::collections::HashMap;
use std::io::Cursor;
use unreal_asset::engine_version::EngineVersion;
use unreal_asset::exports::Export;
use unreal_asset::kismet::*;
use unreal_asset::reader::asset_archive_writer::AssetArchiveWriter;
use unreal_asset::reader::raw_writer::RawWriter;

pub use print::disassemble;

pub type Asset = unreal_asset::Asset<Cursor<Vec<u8>>>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not read asset: {0}")]
    Asset(String),
    #[error("{0}")]
    Parse(#[from] parse::ParseError),
    #[error("{0}")]
    Msg(String),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Msg(s)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Files {
    pub uasset: Vec<u8>,
    pub uexp: Vec<u8>,
}

impl Files {
    pub fn read(path: &std::path::Path) -> std::io::Result<Files> {
        let uasset = std::fs::read(path)?;
        let uexp = std::fs::read(path.with_extension("uexp")).unwrap_or_default();
        Ok(Files { uasset, uexp })
    }

    pub fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, &self.uasset)?;
        std::fs::write(path.with_extension("uexp"), &self.uexp)
    }
}

pub fn load(f: &Files) -> Result<Asset> {
    Asset::new(
        Cursor::new(f.uasset.clone()),
        Some(Cursor::new(f.uexp.clone())),
        EngineVersion::VER_UE4_27,
    )
    .map_err(|e| Error::Asset(e.to_string()))
}

pub fn save(a: &Asset) -> Result<Files> {
    let mut ua = Cursor::new(Vec::new());
    let mut ue = Cursor::new(Vec::new());
    a.write_data(&mut ua, Some(&mut ue))
        .map_err(|e| Error::Asset(e.to_string()))?;
    Ok(Files {
        uasset: ua.into_inner(),
        uexp: ue.into_inner(),
    })
}

pub fn measure(asset: &Asset, e: &KE) -> std::result::Result<usize, String> {
    let mut buf = Cursor::new(Vec::<u8>::new());
    let mut raw = RawWriter::new(
        &mut buf,
        asset.asset_data.object_version,
        asset.asset_data.object_version_ue5,
        asset.asset_data.use_event_driven_loader,
        asset.get_name_map(),
    );
    let mut w = AssetArchiveWriter::new(
        &mut raw,
        &asset.asset_data,
        &asset.imports,
        asset.get_name_map(),
    );
    KE::write(e, &mut w).map_err(|e| e.to_string())
}

pub(crate) fn export_code_mut(a: &mut Asset, idx: i32) -> Option<&mut Option<Vec<KE>>> {
    let e = a.asset_data.exports.get_mut((idx - 1) as usize)?;
    match e {
        Export::ClassExport(c) => Some(&mut c.struct_export.script_bytecode),
        Export::FunctionExport(f) => Some(&mut f.struct_export.script_bytecode),
        Export::StructExport(s) => Some(&mut s.script_bytecode),
        _ => None,
    }
}

pub struct Compiled {
    pub code: Vec<(i32, Vec<KE>)>,
    pub labels: HashMap<i32, HashMap<String, u32>>,
}

pub fn function_index(low: &Lower, f: &Function) -> std::result::Result<i32, String> {
    let i = low.objs.resolve_one(&f.name, Want::Export)?;
    if export_struct(low.asset, i).is_none() {
        return Err(format!("{} is not a function", f.name));
    }
    Ok(i)
}

fn export_struct(a: &Asset, i: i32) -> Option<&unreal_asset::exports::struct_export::StructExport> {
    lower::struct_parts(a.asset_data.exports.get((i - 1) as usize)?)
}

fn layout(
    low: &mut Lower,
    body: &[Stmt],
    noend: bool,
) -> std::result::Result<(HashMap<String, u32>, u32), String> {
    let mut labels = HashMap::new();
    let mut pos = 0;
    for (k, s) in body.iter().enumerate() {
        match s {
            Stmt::Label(l) => {
                if labels.insert(l.clone(), pos).is_some() {
                    return Err(format!("label {l} defined twice"));
                }
            }
            s => {
                let e = low
                    .stmt(s, pos)
                    .map_err(|e| format!("statement {}: {e}", k + 1))?
                    .expect("not a label");
                pos += low.size(&e)?;
            }
        }
    }
    if !noend {
        pos += 1;
    }
    Ok((labels, pos))
}

pub fn compile(low: &mut Lower, funcs: &[Function]) -> std::result::Result<Compiled, String> {
    let mut idx = vec![];
    for f in funcs {
        let i = function_index(low, f)?;
        if idx.contains(&i) {
            return Err(format!("function {} appears twice", f.name));
        }
        idx.push(i);
    }
    low.dummy = true;
    let mut labels = HashMap::new();
    for (f, &i) in funcs.iter().zip(&idx) {
        low.set_function(i);
        let (l, _) = layout(low, &f.body, f.noend).map_err(|e| format!("{}: {e}", f.name))?;
        labels.insert(i, l);
    }
    low.dummy = false;
    let mut code = vec![];
    for (f, &i) in funcs.iter().zip(&idx) {
        low.set_function(i);
        low.labels = labels[&i].clone();
        low.uber_labels = labels.get(&low.uber).cloned().unwrap_or_default();
        let mut out = vec![];
        let mut pos = 0;
        for (k, s) in f.body.iter().enumerate() {
            if let Some(e) = low
                .stmt(s, pos)
                .map_err(|e| format!("{}: statement {}: {e}", f.name, k + 1))?
            {
                pos += low.size(&e)?;
                out.push(e);
            } else if let Stmt::Label(l) = s {
                if labels[&i][l] != pos {
                    return Err(format!("{}: label {l} moved while assembling", f.name));
                }
            }
        }
        if !f.noend {
            out.push(ke!(ExEndOfScript));
        }
        code.push((i, out));
    }
    Ok(Compiled { code, labels })
}

pub fn assemble(a: &mut Asset, src: &str) -> Result<usize> {
    let items = parse::parse(src)?;
    let mut funcs = vec![];
    let mut imports = vec![];
    for it in items {
        match it {
            Item::Function(f) => funcs.push(f),
            Item::Import(r) => imports.push(r),
            Item::Patch(_) => {
                return Err(Error::Msg(
                    "patch declarations need 'bpscript patch'".into(),
                ))
            }
        }
    }
    let (compiled, new_imports) = {
        let mut low = Lower::new(a, true);
        for r in &imports {
            low.object(r, Want::Any)?;
        }
        let c = compile(&mut low, &funcs)?;
        (c, low.objs.new_imports)
    };
    a.imports.extend(new_imports);
    let n = compiled.code.len();
    for (i, code) in compiled.code {
        *export_code_mut(a, i).expect("checked") = Some(code);
    }
    Ok(n)
}
