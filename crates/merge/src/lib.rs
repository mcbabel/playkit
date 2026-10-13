mod code;
mod contains;
mod refs;

use code::{CodeDiff, Relocate, Shift};
pub use contains::contains;
use refs::IndexMap;
use std::collections::HashMap;
use std::io::Cursor;
use unreal_asset::engine_version::EngineVersion;
use unreal_asset::exports::{Export, ExportBaseTrait, ExportNormalTrait};
use unreal_asset::properties::{Property, PropertyDataTrait};
use unreal_asset::types::PackageIndex;
use unreal_asset::Import;

pub type Asset = unreal_asset::Asset<Cursor<Vec<u8>>>;

#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("could not read asset: {0}")]
    Asset(String),
    #[error("cannot combine automatically: {0}")]
    Unsupported(String),
    #[error("conflict: {0}")]
    Conflict(String),
}

pub type Result<T> = std::result::Result<T, MergeError>;

pub struct Files {
    pub uasset: Vec<u8>,
    pub uexp: Vec<u8>,
}

pub fn load(f: &Files) -> Result<Asset> {
    Asset::new(
        Cursor::new(f.uasset.clone()),
        Some(Cursor::new(f.uexp.clone())),
        EngineVersion::VER_UE4_27,
    )
    .map_err(|e| MergeError::Asset(e.to_string()))
}

pub fn save(a: &mut Asset) -> Result<Files> {
    let mut ua = Cursor::new(Vec::new());
    let mut ue = Cursor::new(Vec::new());
    a.write_data(&mut ua, Some(&mut ue))
        .map_err(|e| MergeError::Asset(e.to_string()))?;
    Ok(Files {
        uasset: ua.into_inner(),
        uexp: ue.into_inner(),
    })
}

pub fn merge(base: &Files, mods: &[Files]) -> Result<Files> {
    let (first, rest) = mods
        .split_first()
        .ok_or_else(|| MergeError::Unsupported("nothing to merge".into()))?;
    if rest.is_empty() {
        return Ok(Files {
            uasset: first.uasset.clone(),
            uexp: first.uexp.clone(),
        });
    }
    let base = load(base)?;
    let mut out = load(first)?;
    for m in rest {
        let v = load(m)?;
        apply(&base, &v, &mut out)?;
    }
    save(&mut out)
}

fn exports(a: &Asset) -> &Vec<Export> {
    &a.asset_data.exports
}

fn export_name(e: &Export) -> String {
    e.get_base_export().object_name.get_content()
}

fn same_import(a: &Import, b: &Import) -> bool {
    a.class_package.eq_content(&b.class_package)
        && a.class_name.eq_content(&b.class_name)
        && a.object_name.eq_content(&b.object_name)
        && a.outer_index == b.outer_index
}

fn check_prefix(base: &Asset, other: &Asset, label: &str) -> Result<()> {
    let rewritten = || {
        MergeError::Unsupported(format!(
            "{label} was saved by a different tool and cannot be combined"
        ))
    };
    if other.imports.len() < base.imports.len() || exports(other).len() < exports(base).len() {
        return Err(rewritten());
    }
    if !base
        .imports
        .iter()
        .zip(&other.imports)
        .all(|(a, b)| same_import(a, b))
    {
        return Err(rewritten());
    }
    let same_exports = exports(base).iter().zip(exports(other)).all(|(a, b)| {
        let (a, b) = (a.get_base_export(), b.get_base_export());
        a.object_name.eq_content(&b.object_name)
            && a.class_index == b.class_index
            && a.outer_index == b.outer_index
    });
    if !same_exports {
        return Err(rewritten());
    }
    Ok(())
}

fn map_imports(base: &Asset, v: &Asset, r: &mut Asset, m: &mut IndexMap) {
    let nb = base.imports.len();
    for i in nb..v.imports.len() {
        let mut imp = v.imports[i].clone();
        refs::intern(&mut imp, r);
        imp.outer_index = m.map(imp.outer_index);
        let found = r.imports.iter().position(|x| same_import(x, &imp));
        let idx = match found {
            Some(p) => -(p as i32) - 1,
            None => r.add_import(imp).index,
        };
        m.imports.insert(-(i as i32) - 1, idx);
    }
}

fn prop_key(p: &Property) -> (String, i32) {
    (p.get_name().get_content(), p.get_duplication_index())
}

fn merge_properties(
    base: &[Property],
    var: &[Property],
    out: &mut Vec<Property>,
    r: &Asset,
    m: &IndexMap,
    what: &str,
) -> Result<()> {
    let find = |list: &[Property], k: &(String, i32)| list.iter().position(|p| &prop_key(p) == k);
    let mut keys: Vec<(String, i32)> = base.iter().map(prop_key).collect();
    for p in var {
        if !keys.contains(&prop_key(p)) {
            keys.push(prop_key(p));
        }
    }
    for k in keys {
        let (b, v) = (
            find(base, &k).map(|i| &base[i]),
            find(var, &k).map(|i| &var[i]),
        );
        let unchanged = match (b, v) {
            (Some(b), Some(v)) => refs::same(b, v),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            continue;
        }
        let ri = find(out, &k);
        let r_is_base = match (b, ri) {
            (Some(b), Some(i)) => refs::same(b, &out[i]),
            (None, None) => true,
            _ => false,
        };
        let r_is_var = match (v, ri) {
            (Some(v), Some(i)) => {
                let mut v = v.clone();
                refs::intern(&mut v, r);
                refs::remap_property(&mut v, m);
                refs::same(&v, &out[i])
            }
            (None, None) => true,
            _ => false,
        };
        if r_is_var {
            continue;
        }
        if !r_is_base {
            return Err(MergeError::Conflict(format!(
                "{what}: two mods change the property {}",
                k.0
            )));
        }
        match (v, ri) {
            (Some(v), ri) => {
                let mut v = v.clone();
                refs::intern(&mut v, r);
                refs::remap_property(&mut v, m);
                match ri {
                    Some(i) => out[i] = v,
                    None => out.push(v),
                }
            }
            (None, Some(i)) => {
                out.remove(i);
            }
            (None, None) => {}
        }
    }
    Ok(())
}

fn struct_parts(e: &Export) -> Option<&unreal_asset::exports::struct_export::StructExport> {
    match e {
        Export::ClassExport(c) => Some(&c.struct_export),
        Export::FunctionExport(f) => Some(&f.struct_export),
        Export::StructExport(s) => Some(s),
        _ => None,
    }
}

fn struct_parts_mut(
    e: &mut Export,
) -> Option<&mut unreal_asset::exports::struct_export::StructExport> {
    match e {
        Export::ClassExport(c) => Some(&mut c.struct_export),
        Export::FunctionExport(f) => Some(&mut f.struct_export),
        Export::StructExport(s) => Some(s),
        _ => None,
    }
}

fn apply(base: &Asset, v: &Asset, r: &mut Asset) -> Result<()> {
    check_prefix(base, v, "a mod")?;
    check_prefix(base, r, "a mod")?;
    let mut m = IndexMap {
        base_imports: base.imports.len() as i32,
        base_exports: exports(base).len() as i32,
        ..Default::default()
    };
    map_imports(base, v, r, &mut m);
    let first_new = exports(r).len() as i32;
    for (k, i) in (exports(base).len()..exports(v).len()).enumerate() {
        m.exports.insert(i as i32 + 1, first_new + k as i32 + 1);
    }

    let mut diffs: HashMap<usize, CodeDiff> = HashMap::new();
    let mut rewritten: Vec<usize> = Vec::new();
    let mut shifts: HashMap<i32, Shift> = HashMap::new();
    for (i, be) in exports(base).iter().enumerate() {
        let (Some(bs), Some(vs), Some(rs)) = (
            struct_parts(be),
            struct_parts(&exports(v)[i]),
            struct_parts(&exports(r)[i]),
        ) else {
            continue;
        };
        let (Some(bc), Some(vc), Some(rc)) = (
            &bs.script_bytecode,
            &vs.script_bytecode,
            &rs.script_bytecode,
        ) else {
            if bs.script_bytecode_raw != vs.script_bytecode_raw {
                return Err(MergeError::Unsupported(format!(
                    "{}: unreadable code",
                    export_name(be)
                )));
            }
            continue;
        };
        if bc.is_empty() || vc.is_empty() || rc.is_empty() {
            if !(bc.is_empty() && vc.is_empty()) {
                return Err(MergeError::Unsupported(format!(
                    "{}: code added to an empty function",
                    export_name(be)
                )));
            }
            continue;
        }
        let end_base = code::end_of(base, bc)?;
        let end_r = code::end_of(r, rc)?;
        shifts.insert(
            i as i32 + 1,
            Shift {
                end_base,
                delta: end_r as i64 - end_base as i64,
            },
        );
        match code::diff(base, bc, v, vc, &export_name(be)) {
            Ok(d) if d.is_empty() => {}
            Ok(d) => {
                diffs.insert(i, d);
            }
            Err(MergeError::Unsupported(why)) => {
                let untouched =
                    bc.len() == rc.len() && bc.iter().zip(rc).all(|(x, y)| refs::same(x, y));
                if !untouched {
                    return Err(MergeError::Conflict(format!(
                        "{} is rewritten by one mod and changed by another ({why})",
                        export_name(be)
                    )));
                }
                rewritten.push(i);
            }
            Err(e) => return Err(e),
        }
    }

    for i in 0..exports(base).len() {
        let what = export_name(&exports(base)[i]);
        let be = exports(base)[i].clone();
        let ve = exports(v)[i].clone();
        if let Some(d) = diffs.get(&i) {
            let own = &shifts[&(i as i32 + 1)];
            let mut reloc = Relocate {
                own,
                functions: &shifts,
            };
            let rc = struct_parts(&exports(r)[i])
                .and_then(|s| s.script_bytecode.clone())
                .expect("checked above");
            let merged = code::apply(d, r, &rc, &m, &mut reloc, &what)?;
            struct_parts_mut(&mut r.asset_data.exports[i])
                .expect("struct")
                .script_bytecode = Some(merged);
        }
        if rewritten.contains(&i) {
            let none = Shift {
                end_base: usize::MAX,
                delta: 0,
            };
            let mut reloc = Relocate {
                own: &none,
                functions: &shifts,
            };
            let vs = struct_parts(&ve).expect("struct");
            let code: Vec<_> = vs
                .script_bytecode
                .as_ref()
                .expect("checked above")
                .iter()
                .map(|e| code::transplant(e, r, &m, &mut reloc))
                .collect();
            let props: Vec<_> = vs
                .loaded_properties
                .iter()
                .map(|p| {
                    let mut p = p.clone();
                    refs::intern(&mut p, r);
                    refs::remap_fprop(&mut p, &m);
                    p
                })
                .collect();
            let rs = struct_parts_mut(&mut r.asset_data.exports[i]).expect("struct");
            rs.script_bytecode = Some(code);
            rs.loaded_properties = props;
            continue;
        }
        merge_export(&be, &ve, i, r, &m, &what)?;
    }

    for i in exports(base).len()..exports(v).len() {
        let mut e = exports(v)[i].clone();
        refs::intern(&mut e, r);
        refs::remap_export(&mut e, &m);
        if let Some(s) = struct_parts_mut(&mut e) {
            if let Some(code) = &mut s.script_bytecode {
                let none = Shift {
                    end_base: usize::MAX,
                    delta: 0,
                };
                let mut reloc = Relocate {
                    own: &none,
                    functions: &shifts,
                };
                for x in code.iter_mut() {
                    unreal_asset::kismet::walk::Walk::walk(x, &mut reloc);
                }
            }
        }
        r.asset_data.exports.push(e);
    }
    Ok(())
}

fn same_content(a: &Export, b: &Export) -> bool {
    let strip = |e: &Export| {
        let mut e = e.clone();
        let base = e.get_base_export_mut();
        base.serial_offset = 0;
        base.serial_size = 0;
        e
    };
    refs::same(&strip(a), &strip(b))
}

fn merge_list(
    base: &[PackageIndex],
    var: &[PackageIndex],
    out: &mut Vec<PackageIndex>,
    m: &IndexMap,
) {
    for x in var {
        if !base.contains(x) {
            let x = m.map(*x);
            if !out.contains(&x) {
                out.push(x);
            }
        }
    }
}

fn merge_export(
    be: &Export,
    ve: &Export,
    i: usize,
    r: &mut Asset,
    m: &IndexMap,
    what: &str,
) -> Result<()> {
    let (bb, vb) = (be.get_base_export(), ve.get_base_export());
    {
        let rb = r.asset_data.exports[i].get_base_export_mut();
        merge_list(
            &bb.serialization_before_serialization_dependencies,
            &vb.serialization_before_serialization_dependencies,
            &mut rb.serialization_before_serialization_dependencies,
            m,
        );
        merge_list(
            &bb.create_before_serialization_dependencies,
            &vb.create_before_serialization_dependencies,
            &mut rb.create_before_serialization_dependencies,
            m,
        );
        merge_list(
            &bb.serialization_before_create_dependencies,
            &vb.serialization_before_create_dependencies,
            &mut rb.serialization_before_create_dependencies,
            m,
        );
        merge_list(
            &bb.create_before_create_dependencies,
            &vb.create_before_create_dependencies,
            &mut rb.create_before_create_dependencies,
            m,
        );
        if bb.object_flags != vb.object_flags {
            if rb.object_flags != bb.object_flags && rb.object_flags != vb.object_flags {
                return Err(MergeError::Conflict(format!("{what}: object flags")));
            }
            rb.object_flags = vb.object_flags;
        }
    }

    if let (Some(bn), Some(vn)) = (be.get_normal_export(), ve.get_normal_export()) {
        let snapshot = r.asset_data.exports[i]
            .get_normal_export()
            .map(|n| n.properties.clone())
            .unwrap_or_default();
        let mut props = snapshot;
        merge_properties(&bn.properties, &vn.properties, &mut props, r, m, what)?;
        let rn = r.asset_data.exports[i]
            .get_normal_export_mut()
            .expect("normal export");
        rn.properties = props;
        if bn.extras != vn.extras {
            if rn.extras != bn.extras && rn.extras != vn.extras {
                return Err(MergeError::Conflict(format!("{what}: extra data")));
            }
            rn.extras = vn.extras.clone();
        }
    } else if !same_content(be, ve) {
        return Err(MergeError::Unsupported(format!(
            "{what}: this kind of object cannot be combined"
        )));
    }

    if let (Some(bs), Some(vs)) = (struct_parts(be), struct_parts(ve)) {
        let mut new_props = Vec::new();
        for p in &vs.loaded_properties {
            let name = refs::fprop_name(p);
            let in_base = bs
                .loaded_properties
                .iter()
                .any(|b| refs::fprop_name(b) == name);
            if !in_base {
                let mut p = p.clone();
                refs::intern(&mut p, r);
                refs::remap_fprop(&mut p, m);
                new_props.push((name, p));
            }
        }
        if bs.loaded_properties.len() + new_props.len() != vs.loaded_properties.len() {
            return Err(MergeError::Unsupported(format!(
                "{what}: variables were changed or removed"
            )));
        }
        let rs = struct_parts_mut(&mut r.asset_data.exports[i]).expect("struct");
        for (name, p) in new_props {
            let existing = rs
                .loaded_properties
                .iter()
                .find(|x| refs::fprop_name(x) == name);
            match existing {
                Some(x) if refs::same(x, &p) => {}
                Some(_) => {
                    return Err(MergeError::Conflict(format!(
                        "{what}: two mods add the variable {name}"
                    )))
                }
                None => rs.loaded_properties.push(p),
            }
        }
        let mut children = rs.children.clone();
        merge_list(&bs.children, &vs.children, &mut children, m);
        rs.children = children;
        if bs.super_struct != vs.super_struct {
            return Err(MergeError::Unsupported(format!(
                "{what}: parent class changed"
            )));
        }
    }

    let new_funcs: Vec<(unreal_asset::types::fname::FName, PackageIndex)> = match (be, ve) {
        (Export::ClassExport(bc), Export::ClassExport(vc)) => vc
            .func_map
            .iter()
            .filter(|(_, k, _)| !bc.func_map.iter().any(|(_, bk, _)| bk.eq_content(k)))
            .map(|(_, k, v)| {
                let mut k = k.clone();
                refs::intern(&mut k, r);
                (k, m.map(*v))
            })
            .collect(),
        _ => Vec::new(),
    };
    match (be, ve, &mut r.asset_data.exports[i]) {
        (Export::ClassExport(bc), Export::ClassExport(vc), Export::ClassExport(rc)) => {
            for (key, val) in new_funcs {
                if !rc.func_map.iter().any(|(_, rk, _)| rk.eq_content(&key)) {
                    rc.func_map.insert(key, val);
                }
            }
            if bc.class_flags != vc.class_flags {
                rc.class_flags = vc.class_flags;
            }
        }
        (Export::FunctionExport(bf), Export::FunctionExport(vf), Export::FunctionExport(rf))
            if bf.function_flags != vf.function_flags =>
        {
            if rf.function_flags != bf.function_flags && rf.function_flags != vf.function_flags {
                return Err(MergeError::Conflict(format!("{what}: function flags")));
            }
            rf.function_flags = vf.function_flags;
        }
        _ => {}
    }
    Ok(())
}
