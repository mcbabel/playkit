use crate::{code, load, refs, Asset, Files, Result};
use unreal_asset::exports::{Export, ExportBaseTrait, ExportNormalTrait};
use unreal_asset::properties::{Property, PropertyDataTrait};
use unreal_asset::types::PackageIndex;

fn class_name(a: &Asset, i: PackageIndex) -> String {
    if i.is_import() {
        a.imports
            .get((-i.index - 1) as usize)
            .map(|x| x.object_name.get_content())
            .unwrap_or_default()
    } else if i.is_export() {
        a.asset_data
            .exports
            .get((i.index - 1) as usize)
            .map(|e| e.get_base_export().object_name.get_content())
            .unwrap_or_default()
    } else {
        String::new()
    }
}

fn key(p: &Property) -> (String, i32) {
    (p.get_name().get_content(), p.get_duplication_index())
}

fn struct_parts(e: &Export) -> Option<&unreal_asset::exports::struct_export::StructExport> {
    match e {
        Export::ClassExport(c) => Some(&c.struct_export),
        Export::FunctionExport(f) => Some(&f.struct_export),
        Export::StructExport(s) => Some(s),
        _ => None,
    }
}

/// Whether `current` already carries every change `modded` makes to `base`, even if it also
/// carries changes of other mods or was built by another tool.
pub fn contains(base: &Files, modded: &Files, current: &Files) -> Result<bool> {
    let (o, m, c) = (load(base)?, load(modded)?, load(current)?);
    let (oe, me, ce) = (
        &o.asset_data.exports,
        &m.asset_data.exports,
        &c.asset_data.exports,
    );
    if me.len() < oe.len() || ce.len() < oe.len() || m.imports.len() < o.imports.len() {
        return Ok(false);
    }
    let mut seen = m.imports.len() > o.imports.len() || me.len() > oe.len();
    for imp in &m.imports[o.imports.len()..] {
        let found = c.imports.iter().any(|x| {
            x.class_package.eq_content(&imp.class_package)
                && x.class_name.eq_content(&imp.class_name)
                && x.object_name.eq_content(&imp.object_name)
        });
        if !found {
            return Ok(false);
        }
    }
    for e in &me[oe.len()..] {
        let b = e.get_base_export();
        let cls = class_name(&m, b.class_index);
        let found = ce.iter().any(|x| {
            let xb = x.get_base_export();
            xb.object_name.eq_content(&b.object_name) && class_name(&c, xb.class_index) == cls
        });
        if !found {
            return Ok(false);
        }
    }
    for i in 0..oe.len() {
        let (eo, em, ec) = (&oe[i], &me[i], &ce[i]);
        if !eo
            .get_base_export()
            .object_name
            .eq_content(&ec.get_base_export().object_name)
        {
            return Ok(false);
        }
        if let (Some(no), Some(nm), Some(nc)) = (
            eo.get_normal_export(),
            em.get_normal_export(),
            ec.get_normal_export(),
        ) {
            let find = |list: &[Property], k: &(String, i32), n: usize| {
                list.iter().filter(|p| &key(p) == k).nth(n).cloned()
            };
            for (j, pm) in nm.properties.iter().enumerate() {
                let k = key(pm);
                let n = nm.properties[..j].iter().filter(|p| key(p) == k).count();
                let po = find(&no.properties, &k, n);
                if po.as_ref().is_some_and(|po| refs::same(po, pm)) {
                    continue;
                }
                seen = true;
                match find(&nc.properties, &k, n) {
                    None => return Ok(false),
                    Some(pc) if refs::same(&pc, pm) => {}
                    Some(pc) if po.as_ref().is_some_and(|po| !refs::same(po, &pc)) => {}
                    Some(_) => return Ok(false),
                }
            }
        }
        if let (Some(so), Some(sm), Some(sc)) =
            (struct_parts(eo), struct_parts(em), struct_parts(ec))
        {
            for p in &sm.loaded_properties {
                let name = refs::fprop_name(p);
                let new = !so
                    .loaded_properties
                    .iter()
                    .any(|x| refs::fprop_name(x) == name);
                seen |= new;
                if new
                    && !sc
                        .loaded_properties
                        .iter()
                        .any(|x| refs::fprop_name(x) == name)
                {
                    return Ok(false);
                }
            }
            if let (Some(bc), Some(vc), Some(cc)) = (
                &so.script_bytecode,
                &sm.script_bytecode,
                &sc.script_bytecode,
            ) {
                if bc.is_empty() || vc.is_empty() || cc.is_empty() {
                    continue;
                }
                match code::diff(&o, bc, &m, vc, "check") {
                    Ok(d) => {
                        if d.is_empty() {
                            continue;
                        }
                        seen = true;
                        let co = code::offsets(&c, cc)?;
                        for edit in &d.edits {
                            let at = co.iter().position(|&x| x == edit.off);
                            let untouched =
                                at.is_some_and(|j| j < cc.len() && refs::same(&cc[j], &edit.base));
                            if at.is_none() || untouched {
                                return Ok(false);
                            }
                        }
                        if !d.tail.is_empty() && code::end_of(&c, cc)? <= d.end_base {
                            return Ok(false);
                        }
                    }
                    Err(_) => {
                        seen = true;
                        let unchanged = bc.len() == cc.len()
                            && bc.iter().zip(cc).all(|(x, y)| refs::same(x, y));
                        if unchanged {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        if let (Export::ClassExport(xo), Export::ClassExport(xm), Export::ClassExport(xc)) =
            (eo, em, ec)
        {
            for (_, k, _) in xm.func_map.iter() {
                let new = !xo.func_map.iter().any(|(_, ok, _)| ok.eq_content(k));
                seen |= new;
                if new && !xc.func_map.iter().any(|(_, ck, _)| ck.eq_content(k)) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(seen || (modded.uasset == current.uasset && modded.uexp == current.uexp))
}
