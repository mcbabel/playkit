use crate::refs::{self, IndexMap};
use crate::{Asset, MergeError, Result};
use std::collections::HashMap;
use std::io::Cursor;
use unreal_asset::kismet::walk::{Visitor, Walk};
use unreal_asset::kismet::{ExNothing, KismetExpression};
use unreal_asset::reader::asset_archive_writer::AssetArchiveWriter;
use unreal_asset::reader::raw_writer::RawWriter;

type KE = KismetExpression;

pub fn measure(asset: &Asset, e: &KE) -> Result<usize> {
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
    KE::write(e, &mut w).map_err(|e| MergeError::Asset(e.to_string()))
}

pub fn offsets(asset: &Asset, code: &[KE]) -> Result<Vec<usize>> {
    let mut out = Vec::with_capacity(code.len() + 1);
    let mut off = 0;
    for e in code {
        out.push(off);
        off += measure(asset, e)?;
    }
    out.push(off);
    Ok(out)
}

pub fn end_of(asset: &Asset, code: &[KE]) -> Result<usize> {
    if !matches!(code.last(), Some(KE::ExEndOfScript(_))) {
        return Err(MergeError::Unsupported(
            "function does not end with EndOfScript".into(),
        ));
    }
    Ok(offsets(asset, code)?[code.len() - 1])
}

pub struct Edit {
    pub off: usize,
    pub len: usize,
    pub base: KE,
    pub new: Vec<KE>,
}

pub struct CodeDiff {
    pub end_base: usize,
    pub edits: Vec<Edit>,
    pub tail: Vec<KE>,
}

impl CodeDiff {
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty() && self.tail.is_empty()
    }
}

pub fn diff(base: &Asset, bcode: &[KE], var: &Asset, vcode: &[KE], what: &str) -> Result<CodeDiff> {
    let bo = offsets(base, bcode)?;
    let vo = offsets(var, vcode)?;
    let end_base = end_of(base, bcode)?;
    end_of(var, vcode)?;
    let unsupported = |why: &str| MergeError::Unsupported(format!("{what}: {why}"));
    let mut edits = Vec::new();
    let mut j = 0;
    for i in 0..bcode.len() - 1 {
        let (off, next) = (bo[i], bo[i + 1]);
        while j < vcode.len() && vo[j] < off {
            j += 1;
        }
        if j >= vcode.len() || vo[j] != off {
            return Err(unsupported("code was rewritten, not extended"));
        }
        let start = j;
        while j < vcode.len() && vo[j] < next {
            j += 1;
        }
        if vo[j] != next {
            return Err(unsupported("an edit does not keep the original layout"));
        }
        let new = &vcode[start..j];
        if new.len() == 1 && refs::same(&new[0], &bcode[i]) {
            continue;
        }
        edits.push(Edit {
            off,
            len: next - off,
            base: bcode[i].clone(),
            new: new.to_vec(),
        });
    }
    if vo[j] != end_base {
        return Err(unsupported("code before the end changed size"));
    }
    let tail = vcode[j..vcode.len() - 1].to_vec();
    Ok(CodeDiff {
        end_base,
        edits,
        tail,
    })
}

pub struct Shift {
    pub end_base: usize,
    pub delta: i64,
}

pub struct Relocate<'a> {
    pub own: &'a Shift,
    pub functions: &'a HashMap<i32, Shift>,
}

impl Relocate<'_> {
    fn fix(&self, v: &mut u32) {
        if *v as usize >= self.own.end_base {
            *v = (*v as i64 + self.own.delta) as u32;
        }
    }

    fn entry(&self, node: i32, params: &mut [KE]) {
        if let (Some(s), Some(KE::ExIntConst(i))) = (self.functions.get(&node), params.first_mut())
        {
            if i.value >= 0 && i.value as usize >= s.end_base {
                i.value = (i.value as i64 + s.delta) as i32;
            }
        }
    }
}

impl Visitor for Relocate<'_> {
    fn expr(&mut self, e: &mut KE) {
        match e {
            KE::ExJump(j) => self.fix(&mut j.code_offset),
            KE::ExJumpIfNot(j) => self.fix(&mut j.code_offset),
            KE::ExPushExecutionFlow(p) => self.fix(&mut p.pushing_address),
            KE::ExSkipOffsetConst(s) => self.fix(&mut s.value),
            KE::ExSwitchValue(s) => {
                self.fix(&mut s.end_goto_offset);
                for c in &mut s.cases {
                    self.fix(&mut c.next_offset);
                }
            }
            KE::ExLocalFinalFunction(f) => self.entry(f.stack_node.index, &mut f.parameters),
            KE::ExFinalFunction(f) => self.entry(f.stack_node.index, &mut f.parameters),
            _ => {}
        }
    }
}

pub fn transplant(e: &KE, target: &Asset, map: &IndexMap, reloc: &mut Relocate) -> KE {
    let mut e = e.clone();
    refs::intern(&mut e, target);
    refs::remap_expr(&mut e, map);
    e.walk(reloc);
    e
}

fn nothing() -> KE {
    KE::ExNothing(ExNothing::default())
}

pub fn apply(
    d: &CodeDiff,
    target: &Asset,
    rcode: &[KE],
    map: &IndexMap,
    reloc: &mut Relocate,
    what: &str,
) -> Result<Vec<KE>> {
    let mut code = rcode.to_vec();
    for edit in &d.edits {
        let new: Vec<KE> = edit
            .new
            .iter()
            .map(|e| transplant(e, target, map, reloc))
            .collect();
        let ro = offsets(target, &code)?;
        let at = ro.iter().position(|&o| o == edit.off);
        let fits =
            at.is_some_and(|i| ro[i + 1] - ro[i] == edit.len && refs::same(&code[i], &edit.base));
        if fits {
            let i = at.expect("checked");
            code.splice(i..i + 1, new);
            continue;
        }
        let end_r = end_of(target, &code)?;
        let next = (edit.off + edit.len) as u32;
        let copies: Vec<usize> = (0..code.len() - 1)
            .filter(|&i| ro[i] >= d.end_base.min(end_r) && ro[i] >= edit.off + edit.len)
            .filter(|&i| ro[i + 1] - ro[i] == edit.len && refs::same(&code[i], &edit.base))
            .filter(|&i| matches!(&code[i + 1], KE::ExJump(j) if j.code_offset == next))
            .collect();
        match copies.as_slice() {
            [i] => {
                let i = *i;
                code.splice(i..i + 1, new);
            }
            _ => {
                return Err(MergeError::Conflict(format!(
                    "{what}: two mods change the same instruction at {}",
                    edit.off
                )))
            }
        }
    }
    let end = code.pop().unwrap_or_else(nothing);
    code.extend(d.tail.iter().map(|e| transplant(e, target, map, reloc)));
    code.push(end);
    Ok(code)
}
