use crate::ast::*;
use crate::lower::{fprop_generic, ke, kptr, struct_parts, Lower, KE, R};
use crate::names::{fname_text, Want};
use crate::parse::{Item, PResult, Parser, Tok};
use crate::print::offsets;
use crate::Asset;
use std::collections::HashMap;
use std::fmt::Write;
use unreal_asset::enums::{EArrayDim, ELifetimeCondition};
use unreal_asset::exports::{Export, ExportBaseTrait};
use unreal_asset::flags::{EFunctionFlags, EObjectFlags, EPropertyFlags};
use unreal_asset::fproperty::*;
use unreal_asset::kismet::walk::{Visitor, Walk};
use unreal_asset::kismet::*;
use unreal_asset::types::fname::ToSerializedName;
use unreal_asset::types::PackageIndex;

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Bool,
    Byte,
    Int,
    Int64,
    Float,
    Str,
    Name,
    Text,
    Object(Ref),
    Class(Ref),
    Struct(Ref, Option<i32>),
    Delegate(Ref),
    Multicast(Ref),
    Array(Box<Ty>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Value,
    ConstRef,
    Ref,
    Out,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Name,
    pub ty: Ty,
    pub mode: Mode,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Body {
    pub locals: Vec<(Name, Ty)>,
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HookKind {
    Before,
    After,
    Replace,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchItem {
    Var {
        class: Option<Ref>,
        name: Name,
        ty: Ty,
    },
    Local {
        func: Ref,
        name: Name,
        ty: Ty,
    },
    Function {
        name: Name,
        params: Vec<Param>,
        ret: Option<Ty>,
        body: Body,
    },
    Event {
        name: Name,
        params: Vec<Param>,
        body: Body,
    },
    Hook {
        kind: HookKind,
        func: Ref,
        uber: bool,
        body: Body,
    },
}

fn parse_ty(p: &mut Parser) -> PResult<Ty> {
    let word = match p.peek() {
        Tok::Ident(s) if !s.starts_with('`') => s.clone(),
        _ => return p.err("expected a type"),
    };
    p.bump();
    let generic = |p: &mut Parser| -> PResult<Ref> {
        p.expect("<")?;
        let r = p.reference()?;
        p.expect(">")?;
        Ok(r)
    };
    Ok(match word.as_str() {
        "bool" => Ty::Bool,
        "byte" => Ty::Byte,
        "int" => Ty::Int,
        "int64" => Ty::Int64,
        "float" => Ty::Float,
        "string" => Ty::Str,
        "name" => Ty::Name,
        "text" => Ty::Text,
        "object" => Ty::Object(generic(p)?),
        "class" => Ty::Class(generic(p)?),
        "delegate" => Ty::Delegate(generic(p)?),
        "multicast" => Ty::Multicast(generic(p)?),
        "struct" => {
            p.expect("<")?;
            let r = p.reference()?;
            let size = if p.eat(",") {
                Some(p.int()? as i32)
            } else {
                None
            };
            p.expect(">")?;
            Ty::Struct(r, size)
        }
        "array" => {
            p.expect("<")?;
            let t = parse_ty(p)?;
            p.expect(">")?;
            Ty::Array(Box::new(t))
        }
        w => return p.err(format!("unknown type {w}")),
    })
}

fn parse_params(p: &mut Parser) -> PResult<Vec<Param>> {
    p.expect("(")?;
    let mut out = vec![];
    while !p.eat(")") {
        let name = p.name()?;
        p.expect(":")?;
        let mode = if p.eat_kw("const") {
            p.expect_kw("ref")?;
            Mode::ConstRef
        } else if p.eat_kw("ref") {
            Mode::Ref
        } else if p.eat_kw("out") {
            Mode::Out
        } else {
            Mode::Value
        };
        let ty = parse_ty(p)?;
        out.push(Param { name, ty, mode });
        if !p.is_punct(")") {
            p.expect(",")?;
        }
    }
    Ok(out)
}

fn is_decl(p: &Parser) -> bool {
    p.is_kw("local") && matches!(p.peek_at(1), Tok::Ident(_)) && p.peek_at(2) == &Tok::Punct(":")
}

fn parse_body(p: &mut Parser) -> PResult<Body> {
    p.expect("{")?;
    let mut body = Body::default();
    while !p.eat("}") {
        if p.at_eof() {
            return p.err("missing '}'");
        }
        if is_decl(p) {
            p.bump();
            let n = p.name()?;
            p.expect(":")?;
            let t = parse_ty(p)?;
            p.expect(";")?;
            body.locals.push((n, t));
        } else {
            body.stmts.push(p.stmt()?);
        }
    }
    Ok(body)
}

fn segments(p: &mut Parser) -> PResult<Vec<Name>> {
    let mut v = vec![p.name()?];
    while p.eat("::") {
        v.push(p.name()?);
    }
    Ok(v)
}

pub fn parse_item(p: &mut Parser) -> PResult<Option<PatchItem>> {
    let kw = match p.peek() {
        Tok::Ident(s) if !s.starts_with('`') => s.clone(),
        _ => return Ok(None),
    };
    match kw.as_str() {
        "function" if p.peek_at(2) == &Tok::Punct("(") => {
            p.bump();
            let name = p.name()?;
            let params = parse_params(p)?;
            let ret = if p.eat("->") {
                Some(parse_ty(p)?)
            } else {
                None
            };
            let body = parse_body(p)?;
            Ok(Some(PatchItem::Function {
                name,
                params,
                ret,
                body,
            }))
        }
        "event" => {
            p.bump();
            let name = p.name()?;
            let params = parse_params(p)?;
            let body = parse_body(p)?;
            Ok(Some(PatchItem::Event { name, params, body }))
        }
        "var" | "local" => {
            p.bump();
            let mut segs = segments(p)?;
            p.expect(":")?;
            let ty = parse_ty(p)?;
            p.expect(";")?;
            let name = segs.pop().expect("one name");
            if kw == "var" {
                let class = (!segs.is_empty()).then_some(Ref::Path(segs));
                Ok(Some(PatchItem::Var { class, name, ty }))
            } else if segs.is_empty() {
                p.err("a top level local needs the function: local Function::Name: type;")
            } else {
                Ok(Some(PatchItem::Local {
                    func: Ref::Path(segs),
                    name,
                    ty,
                }))
            }
        }
        "before" | "after" | "replace" => {
            p.bump();
            let kind = match kw.as_str() {
                "before" => HookKind::Before,
                "after" => HookKind::After,
                _ => HookKind::Replace,
            };
            let func = p.reference()?;
            let uber = p.is_kw("ubergraph");
            if uber {
                p.bump();
                if kind == HookKind::Replace {
                    return p.err("replace cannot run in the ubergraph");
                }
            }
            let body = parse_body(p)?;
            Ok(Some(PatchItem::Hook {
                kind,
                func,
                uber,
                body,
            }))
        }
        _ => Ok(None),
    }
}

#[derive(Clone, Debug)]
enum RTy {
    Bool,
    Byte,
    Int,
    Int64,
    Float,
    Str,
    Name,
    Text,
    Object(i32),
    Class(i32, i32),
    Struct(i32, i32),
    Delegate(i32),
    Multicast(i32),
    Array(Box<RTy>),
}

fn resolve_ty(low: &mut Lower, t: &Ty) -> R<RTy> {
    Ok(match t {
        Ty::Bool => RTy::Bool,
        Ty::Byte => RTy::Byte,
        Ty::Int => RTy::Int,
        Ty::Int64 => RTy::Int64,
        Ty::Float => RTy::Float,
        Ty::Str => RTy::Str,
        Ty::Name => RTy::Name,
        Ty::Text => RTy::Text,
        Ty::Object(r) => RTy::Object(low.object(r, Want::Class)?),
        Ty::Class(r) => {
            let meta = low.object(r, Want::Class)?;
            let class = low.object(
                &Ref::Quoted {
                    class: None,
                    path: "/Script/CoreUObject.Class".into(),
                },
                Want::Class,
            )?;
            RTy::Class(class, meta)
        }
        Ty::Struct(r, size) => {
            let s = low.object(r, Want::Struct)?;
            let size = match size {
                Some(n) => *n,
                None => low.struct_size(s).ok_or_else(|| {
                    format!("size of struct {r} unknown; write struct<{r}, size>")
                })?,
            };
            RTy::Struct(s, size)
        }
        Ty::Delegate(r) => RTy::Delegate(low.object(r, Want::Function)?),
        Ty::Multicast(r) => RTy::Multicast(low.object(r, Want::Function)?),
        Ty::Array(t) => RTy::Array(Box::new(resolve_ty(low, t)?)),
    })
}

const LOCAL: (u64, u32) = (0, 0x1);
const LOCAL_ARRAY: u64 = 0x800_0000;
const CLASS_VAR: (u64, u32) = (0x10005, 0x200001);
const RETURN_PARM: u64 = 0x580;

fn param_flags(m: Mode) -> u64 {
    match m {
        Mode::Value => 0x94,
        Mode::ConstRef => 0x800_0196,
        Mode::Ref => 0x800_0194,
        Mode::Out => 0x180,
    }
}

fn build_prop(low: &mut Lower, name: &Name, t: &RTy, flags: (u64, u32)) -> R<FProperty> {
    let prop = make_prop(low, name, t, flags)?;
    low.fname(&Name::new(prop.to_serialized_name()))?;
    Ok(prop)
}

fn make_prop(low: &mut Lower, name: &Name, t: &RTy, flags: (u64, u32)) -> R<FProperty> {
    let n = low.fname(name)?;
    let none = low.fname(&Name::new("None"))?;
    let generic = |low: &mut Lower, size: i32, st: Option<&str>| -> R<FGenericProperty> {
        Ok(FGenericProperty {
            name: n.clone(),
            flags: EObjectFlags::from_bits_truncate(flags.1),
            array_dim: EArrayDim::TArray,
            element_size: size,
            property_flags: EPropertyFlags::from_bits_truncate(flags.0),
            rep_index: 0,
            rep_notify_func: none.clone(),
            blueprint_replication_condition: ELifetimeCondition::CondNone,
            serialized_type: match st {
                Some(s) => Some(low.fname(&Name::new(s))?),
                None => None,
            },
        })
    };
    let idx = PackageIndex::new;
    Ok(match t {
        RTy::Bool => FBoolProperty {
            generic_property: generic(low, 1, None)?,
            field_size: 1,
            byte_offset: 0,
            byte_mask: 1,
            field_mask: 0xff,
            native_bool: true,
            value: true,
        }
        .into(),
        RTy::Byte => FByteProperty {
            generic_property: generic(low, 1, None)?,
            enum_value: idx(0),
        }
        .into(),
        RTy::Int => generic(low, 4, Some("IntProperty"))?.into(),
        RTy::Int64 => generic(low, 8, Some("Int64Property"))?.into(),
        RTy::Float => generic(low, 4, Some("FloatProperty"))?.into(),
        RTy::Str => generic(low, 16, Some("StrProperty"))?.into(),
        RTy::Name => generic(low, 12, Some("NameProperty"))?.into(),
        RTy::Text => generic(low, 24, Some("TextProperty"))?.into(),
        RTy::Object(c) => FObjectProperty {
            generic_property: generic(low, 8, None)?,
            property_class: idx(*c),
        }
        .into(),
        RTy::Class(c, m) => FClassProperty {
            generic_property: generic(low, 8, None)?,
            property_class: idx(*c),
            meta_class: idx(*m),
        }
        .into(),
        RTy::Struct(s, size) => FStructProperty {
            generic_property: generic(low, *size, None)?,
            struct_value: idx(*s),
        }
        .into(),
        RTy::Delegate(s) => FDelegateProperty {
            generic_property: generic(low, 20, None)?,
            signature_function: idx(*s),
        }
        .into(),
        RTy::Multicast(s) => FMulticastInlineDelegateProperty {
            generic_property: generic(low, 16, None)?,
            signature_function: idx(*s),
        }
        .into(),
        RTy::Array(inner) => {
            let inner = build_prop(low, name, inner, LOCAL)?;
            let mut g = generic(low, 16, None)?;
            if flags == LOCAL {
                g.property_flags = EPropertyFlags::from_bits_truncate(LOCAL_ARRAY);
            }
            FArrayProperty {
                generic_property: g,
                inner: Box::new(inner),
            }
            .into()
        }
    })
}

fn main_class(a: &Asset) -> R<i32> {
    a.asset_data
        .exports
        .iter()
        .position(|e| matches!(e, Export::ClassExport(_)))
        .map(|i| i as i32 + 1)
        .ok_or_else(|| "the asset has no Blueprint class".to_string())
}

fn ubergraph_of(a: &Asset, class: i32) -> i32 {
    a.asset_data
        .exports
        .iter()
        .enumerate()
        .find(|(_, e)| {
            let b = e.get_base_export();
            matches!(e, Export::FunctionExport(_))
                && b.outer_index.index == class
                && b.object_name.get_content().starts_with("ExecuteUbergraph_")
        })
        .map(|(i, _)| i as i32 + 1)
        .unwrap_or(0)
}

fn struct_mut(
    a: &mut Asset,
    i: i32,
) -> Option<&mut unreal_asset::exports::struct_export::StructExport> {
    match a.asset_data.exports.get_mut((i - 1) as usize)? {
        Export::ClassExport(c) => Some(&mut c.struct_export),
        Export::FunctionExport(f) => Some(&mut f.struct_export),
        Export::StructExport(s) => Some(s),
        _ => None,
    }
}

fn code_of(a: &Asset, i: i32) -> R<Vec<KE>> {
    a.asset_data
        .exports
        .get((i - 1) as usize)
        .and_then(struct_parts)
        .and_then(|s| s.script_bytecode.clone())
        .ok_or_else(|| "function has no readable bytecode".to_string())
}

fn uber_call(uber: i32, entry: u32) -> KE {
    ke!(ExLocalFinalFunction {
        stack_node: PackageIndex::new(uber),
        parameters: vec![ke!(ExIntConst {
            value: entry as i32
        })]
    })
}

fn jump(to: u32) -> KE {
    ke!(ExJump { code_offset: to })
}

fn nothing() -> KE {
    ke!(ExNothing)
}

fn terminator(e: &KE) -> bool {
    matches!(
        e,
        KE::ExJump(_)
            | KE::ExReturn(_)
            | KE::ExPopExecutionFlow(_)
            | KE::ExComputedJump(_)
            | KE::ExEndOfScript(_)
    )
}

struct ShiftSwitch(i64);

impl Visitor for ShiftSwitch {
    fn expr(&mut self, e: &mut KE) {
        if let KE::ExSwitchValue(s) = e {
            s.end_goto_offset = (s.end_goto_offset as i64 + self.0) as u32;
            for c in &mut s.cases {
                c.next_offset = (c.next_offset as i64 + self.0) as u32;
            }
        }
    }
}

struct Retarget(u32, u32);

impl Visitor for Retarget {
    fn expr(&mut self, e: &mut KE) {
        let v = match e {
            KE::ExJump(j) => &mut j.code_offset,
            KE::ExJumpIfNot(j) => &mut j.code_offset,
            KE::ExPushExecutionFlow(p) => &mut p.pushing_address,
            KE::ExSkipOffsetConst(s) => &mut s.value,
            _ => return,
        };
        if *v == self.0 {
            *v = self.1;
        }
    }
}

enum Piece {
    S(Box<Stmt>),
    K(KE, Option<u32>),
}

#[derive(Default)]
struct Block {
    pieces: Vec<Piece>,
    aliases: HashMap<String, String>,
}

fn user(stmts: &[Stmt], in_uber: bool) -> Vec<Piece> {
    stmts
        .iter()
        .map(|s| match s {
            Stmt::Return(None) if in_uber => Piece::S(Box::new(Stmt::Pop)),
            s => Piece::S(Box::new(s.clone())),
        })
        .collect()
}

fn uber_block(stmts: &[Stmt]) -> Block {
    let mut b = Block {
        pieces: user(stmts, true),
        ..Default::default()
    };
    let ends = matches!(
        stmts.last(),
        Some(Stmt::Goto(_) | Stmt::GotoComputed(_) | Stmt::Pop | Stmt::Return(_))
    );
    if !ends {
        b.pieces.push(Piece::S(Box::new(Stmt::Pop)));
    }
    b
}

fn assemble(low: &mut Lower, base: u32, blocks: &[Block], shared: bool) -> R<(Vec<KE>, Vec<u32>)> {
    low.dummy = true;
    let mut pos = base;
    let mut maps: Vec<HashMap<String, u32>> = vec![];
    let mut starts = vec![];
    let mut map = HashMap::new();
    for (k, b) in blocks.iter().enumerate() {
        low.aliases = b.aliases.clone();
        starts.push(pos);
        if !shared {
            map = HashMap::new();
        }
        for (j, p) in b.pieces.iter().enumerate() {
            match p {
                Piece::S(s) => match s.as_ref() {
                    Stmt::Label(l) => {
                        if map.insert(l.clone(), pos).is_some() {
                            return Err(format!("label {l} defined twice"));
                        }
                    }
                    s => {
                        let e = low
                            .stmt(s, pos)
                            .map_err(|e| format!("block {} statement {}: {e}", k + 1, j + 1))?
                            .expect("statement");
                        pos += low.size(&e)?;
                    }
                },
                Piece::K(e, _) => pos += low.size(e)?,
            }
        }
        maps.push(map.clone());
    }
    if shared {
        maps = vec![map; blocks.len()];
    }
    low.dummy = false;
    let mut out = vec![];
    pos = base;
    for (b, map) in blocks.iter().zip(maps) {
        low.aliases = b.aliases.clone();
        low.labels = map;
        for p in &b.pieces {
            let e = match p {
                Piece::S(s) => match low.stmt(s, pos)? {
                    Some(e) => e,
                    None => continue,
                },
                Piece::K(e, orig) => {
                    let mut e = e.clone();
                    if let Some(o) = orig {
                        e.walk(&mut ShiftSwitch(pos as i64 - *o as i64));
                    }
                    e
                }
            };
            pos += low.size(&e)?;
            out.push(e);
        }
    }
    low.aliases.clear();
    Ok((out, starts))
}

fn replace_with_jump(low: &Lower, code: &mut Vec<KE>, i: usize, to: u32) -> R<()> {
    let size = low.size(&code[i])?;
    if size < 5 {
        return Err("statement too small for a jump".into());
    }
    let repl: Vec<KE> = std::iter::once(jump(to))
        .chain((0..size - 5).map(|_| nothing()))
        .collect();
    code.splice(i..i + 1, repl);
    Ok(())
}

#[derive(Default)]
struct Hooks {
    before: Vec<(Body, bool)>,
    after: Vec<(Body, bool)>,
    replace: Option<Body>,
    uber_before: Vec<u32>,
    uber_after: Vec<u32>,
}

struct EventInfo {
    name: String,
    params: Vec<Param>,
    body: Body,
    copies: Vec<String>,
}

struct NewFunc {
    name: String,
    flags: u32,
    props: Vec<FProperty>,
    event: bool,
}

enum UberItem {
    Event(usize),
    Hook(usize, bool, usize),
}

#[derive(Default)]
struct Plan {
    uber_order: Vec<UberItem>,
    props: Vec<(i32, FProperty, String)>,
    funcs: Vec<NewFunc>,
    hooks: Vec<(i32, Hooks)>,
    events: Vec<EventInfo>,
    functions: Vec<(String, Body)>,
}

fn export_function(low: &Lower, r: &Ref) -> R<i32> {
    let i = low.objs.resolve_one(r, Want::Export)?;
    if !matches!(
        low.asset.asset_data.exports[(i - 1) as usize],
        Export::FunctionExport(_)
    ) {
        return Err(format!("{r} is not a function"));
    }
    Ok(i)
}

fn plan(low: &mut Lower, patches: &[PatchItem], class: i32, uber: i32) -> R<Plan> {
    let mut plan = Plan::default();
    let need_uber = |what: &str| -> R<()> {
        if uber == 0 {
            Err(format!("{what} needs an ubergraph, but the class has none"))
        } else {
            Ok(())
        }
    };
    for p in patches {
        match p {
            PatchItem::Var { class: c, name, ty } => {
                let owner = match c {
                    Some(r) => low.objs.resolve_one(r, Want::Export)?,
                    None => class,
                };
                let t = resolve_ty(low, ty)?;
                let prop = build_prop(low, name, &t, CLASS_VAR)?;
                plan.props
                    .push((owner, prop, format!("class variable {}", name.text)));
            }
            PatchItem::Local { func, name, ty } => {
                let f = export_function(low, func)?;
                let t = resolve_ty(low, ty)?;
                let prop = build_prop(low, name, &t, LOCAL)?;
                let what = format!("local {}::{}", low.objs.name(f), name.text);
                plan.props.push((f, prop, what));
            }
            PatchItem::Function {
                name,
                params,
                ret,
                body,
            } => {
                let mut props = vec![];
                let mut out = false;
                for prm in params {
                    let t = resolve_ty(low, &prm.ty)?;
                    props.push(build_prop(low, &prm.name, &t, (param_flags(prm.mode), 1))?);
                    out |= prm.mode != Mode::Value;
                }
                if let Some(rt) = ret {
                    let t = resolve_ty(low, rt)?;
                    props.push(build_prop(
                        low,
                        &Name::new("ReturnValue"),
                        &t,
                        (RETURN_PARM, 1),
                    )?);
                    out = true;
                }
                for (n, ty) in &body.locals {
                    let t = resolve_ty(low, ty)?;
                    props.push(build_prop(low, n, &t, LOCAL)?);
                }
                plan.funcs.push(NewFunc {
                    name: name.text.clone(),
                    flags: 0x0C02_0000 | if out { 0x40_0000 } else { 0 },
                    props,
                    event: false,
                });
                plan.functions.push((name.text.clone(), body.clone()));
            }
            PatchItem::Event { name, params, body } => {
                need_uber("an event")?;
                let mut props = vec![];
                let mut copies = vec![];
                let mut out = false;
                for prm in params {
                    if prm.mode == Mode::Out {
                        return Err(format!(
                            "event {}: events cannot have out parameters",
                            name.text
                        ));
                    }
                    let t = resolve_ty(low, &prm.ty)?;
                    props.push(build_prop(low, &prm.name, &t, (param_flags(prm.mode), 1))?);
                    out |= prm.mode != Mode::Value;
                    let copy = format!("{}_{}", name.text, prm.name.text);
                    let cp = build_prop(low, &Name::new(copy.clone()), &t, LOCAL)?;
                    plan.props
                        .push((uber, cp, format!("ubergraph copy {copy}")));
                    copies.push(copy);
                }
                for (n, ty) in &body.locals {
                    let t = resolve_ty(low, ty)?;
                    let prop = build_prop(low, n, &t, LOCAL)?;
                    plan.props
                        .push((uber, prop, format!("ubergraph local {}", n.text)));
                }
                plan.funcs.push(NewFunc {
                    name: name.text.clone(),
                    flags: 0x0C00_0000 | if out { 0x40_0000 } else { 0 },
                    props,
                    event: true,
                });
                plan.uber_order.push(UberItem::Event(plan.events.len()));
                plan.events.push(EventInfo {
                    name: name.text.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    copies,
                });
            }
            PatchItem::Hook {
                kind,
                func,
                uber: in_uber,
                body,
            } => {
                let f = export_function(low, func)?;
                if f == uber {
                    return Err(
                        "hooks on the ubergraph itself are not supported; use an event".into(),
                    );
                }
                if *in_uber {
                    need_uber("a ubergraph hook")?;
                }
                let owner = if *in_uber { uber } else { f };
                for (n, ty) in &body.locals {
                    let t = resolve_ty(low, ty)?;
                    let prop = build_prop(low, n, &t, LOCAL)?;
                    let what = format!("local {}::{}", low.objs.name(owner), n.text);
                    plan.props.push((owner, prop, what));
                }
                let pos = match plan.hooks.iter().position(|(i, _)| *i == f) {
                    Some(p) => p,
                    None => {
                        plan.hooks.push((f, Hooks::default()));
                        plan.hooks.len() - 1
                    }
                };
                let h = &mut plan.hooks[pos].1;
                if *in_uber {
                    let nth = match kind {
                        HookKind::Before => h.before.iter().filter(|(_, u)| *u).count(),
                        _ => h.after.iter().filter(|(_, u)| *u).count(),
                    };
                    let before = *kind == HookKind::Before;
                    plan.uber_order.push(UberItem::Hook(pos, before, nth));
                }
                match kind {
                    HookKind::Before => h.before.push((body.clone(), *in_uber)),
                    HookKind::After => h.after.push((body.clone(), *in_uber)),
                    HookKind::Replace => {
                        if h.replace.is_some() {
                            return Err(format!("{func} is replaced twice"));
                        }
                        h.replace = Some(body.clone());
                    }
                }
            }
        }
    }
    Ok(plan)
}

fn import_lines(report: &mut String, imports: &[unreal_asset::Import]) {
    for imp in imports {
        let _ = writeln!(
            report,
            "  import {} ({})",
            fname_text(&imp.object_name),
            fname_text(&imp.class_name)
        );
    }
}

pub fn apply(a: &mut Asset, src: &str) -> crate::Result<String> {
    apply_with(a, src, None)
}

/// Like [`apply`], with a reflection database for names the asset does not contain.
pub fn apply_with(
    a: &mut Asset,
    src: &str,
    db: Option<&crate::reflection::Index>,
) -> crate::Result<String> {
    let items = crate::parse::parse(src)?;
    let mut report = String::new();
    let class = main_class(a)?;
    let uber = ubergraph_of(a, class);
    let mut imports = vec![];
    let mut patches = vec![];
    for it in items {
        match it {
            Item::Import(r) => imports.push(r),
            Item::Patch(p) => patches.push(p),
            Item::Function(f) => {
                return Err(format!(
                    "'function {0} {{' is not a patch item; write replace {0} {{ ... }} or function {0}(...) {{ ... }}",
                    f.name
                )
                .into())
            }
        }
    }

    let (mut plan, new_imports) = {
        let mut low = Lower::new(a, true).with_db(db);
        for r in &imports {
            low.object(r, Want::Any)?;
        }
        let plan = plan(&mut low, &patches, class, uber)?;
        (plan, low.objs.new_imports)
    };
    import_lines(&mut report, &new_imports);
    a.imports.extend(new_imports);

    for (owner, prop, what) in std::mem::take(&mut plan.props) {
        let name = fname_text(&fprop_generic(&prop).name);
        let s = struct_mut(a, owner).ok_or_else(|| format!("{what}: not a class or function"))?;
        let exists = s
            .loaded_properties
            .iter()
            .any(|p| fname_text(&fprop_generic(p).name) == name);
        if exists {
            return Err(format!("{what}: a variable with that name already exists").into());
        }
        s.loaded_properties.push(prop);
        let _ = writeln!(report, "  {what}");
    }

    let tpl = template(a, class)?;
    let mut new_index: HashMap<String, i32> = HashMap::new();
    for nf in std::mem::take(&mut plan.funcs) {
        let exists = a.asset_data.exports.iter().any(|e| {
            let b = e.get_base_export();
            b.outer_index.index == class && fname_text(&b.object_name) == nf.name
        });
        if exists {
            return Err(format!("function {} already exists", nf.name).into());
        }
        let owner_uber = if nf.event { uber } else { 0 };
        let i = add_function(a, class, tpl, &nf.name, nf.flags, nf.props, owner_uber)?;
        new_index.insert(nf.name.clone(), i);
        let kind = if nf.event { "event" } else { "function" };
        let _ = writeln!(report, "  {kind} {}", nf.name);
    }

    let (results, more_imports) = {
        let mut low = Lower::new(a, true).with_db(db);
        let results = generate(&mut low, &mut plan, uber, &new_index, &mut report)?;
        (results, low.objs.new_imports)
    };
    import_lines(&mut report, &more_imports);
    a.imports.extend(more_imports);
    for (f, code) in results {
        *crate::export_code_mut(a, f)
            .ok_or_else(|| crate::Error::Msg("not a function".into()))? = Some(code);
    }
    Ok(report)
}

fn generate(
    low: &mut Lower,
    plan: &mut Plan,
    uber: i32,
    new_index: &HashMap<String, i32>,
    report: &mut String,
) -> R<Vec<(i32, Vec<KE>)>> {
    let mut results = vec![];
    let mut entries: HashMap<String, u32> = HashMap::new();
    if uber != 0 {
        let mut blocks = vec![];
        for item in &plan.uber_order {
            blocks.push(match item {
                UberItem::Event(k) => {
                    let ev = &plan.events[*k];
                    let mut b = uber_block(&ev.body.stmts);
                    for (prm, copy) in ev.params.iter().zip(&ev.copies) {
                        b.aliases.insert(prm.name.text.clone(), copy.clone());
                    }
                    b
                }
                UberItem::Hook(k, before, nth) => {
                    let h = &plan.hooks[*k].1;
                    let list = if *before { &h.before } else { &h.after };
                    let body = list
                        .iter()
                        .filter(|(_, u)| *u)
                        .nth(*nth)
                        .map(|(b, _)| b)
                        .expect("ubergraph hook");
                    uber_block(&body.stmts)
                }
            });
        }
        if !blocks.is_empty() {
            let ucode = code_of(low.asset, uber)?;
            if !matches!(ucode.last(), Some(KE::ExEndOfScript(_))) {
                return Err("the ubergraph does not end with EndOfScript".into());
            }
            low.set_function(uber);
            let offs = offsets(low, &ucode)?;
            let base = offs[ucode.len() - 1];
            let (tail, starts) =
                assemble(low, base, &blocks, true).map_err(|e| format!("ubergraph: {e}"))?;
            for (item, start) in plan.uber_order.iter().zip(&starts) {
                match item {
                    UberItem::Event(k) => {
                        entries.insert(plan.events[*k].name.clone(), *start);
                    }
                    UberItem::Hook(k, true, _) => plan.hooks[*k].1.uber_before.push(*start),
                    UberItem::Hook(k, false, _) => plan.hooks[*k].1.uber_after.push(*start),
                }
            }
            let mut code = ucode;
            let end = code.pop().expect("end of script");
            code.extend(tail);
            code.push(end);
            results.push((uber, code));
        }
    }

    for ev in &plan.events {
        let f = new_index[&ev.name];
        let mut code = vec![];
        for (prm, copy) in ev.params.iter().zip(&ev.copies) {
            let pn = low.fname(&prm.name)?;
            let cn = low.fname(&Name::new(copy.clone()))?;
            let p = kptr(f, vec![pn]);
            let src = if prm.mode == Mode::Value {
                ke!(ExLocalVariable { variable: p })
            } else {
                ke!(ExLocalOutVariable { variable: p })
            };
            code.push(ke!(ExLetValueOnPersistentFrame {
                destination_property: kptr(uber, vec![cn]),
                assignment_expression: Box::new(src)
            }));
        }
        code.push(uber_call(uber, entries[&ev.name]));
        code.push(ke!(ExReturn {
            return_expression: Box::new(nothing())
        }));
        code.push(ke!(ExEndOfScript));
        results.push((f, code));
    }

    for (f, h) in &plan.hooks {
        let name = low.objs.name(*f);
        let code = if let Some(body) = &h.replace {
            if !h.before.is_empty() || !h.after.is_empty() {
                return Err(format!(
                    "{name}: replace cannot be combined with before or after"
                ));
            }
            compile_body(low, *f, &body.stmts).map_err(|e| format!("replace {name}: {e}"))?
        } else {
            hook_function(low, *f, uber, h).map_err(|e| format!("{name}: {e}"))?
        };
        results.push((*f, code));
        let what = match (&h.replace, h.before.is_empty(), h.after.is_empty()) {
            (Some(_), _, _) => "replaced",
            (None, false, false) => "hooked before and after",
            (None, false, true) => "hooked before",
            _ => "hooked after",
        };
        let _ = writeln!(report, "  {what} {name}");
    }

    for (name, body) in &plan.functions {
        let f = new_index[name];
        let code =
            compile_body(low, f, &body.stmts).map_err(|e| format!("function {name}: {e}"))?;
        results.push((f, code));
    }
    Ok(results)
}

fn compile_body(low: &mut Lower, f: i32, stmts: &[Stmt]) -> R<Vec<KE>> {
    let mut body = stmts.to_vec();
    if !matches!(body.last(), Some(Stmt::Return(_))) {
        body.push(Stmt::Return(None));
    }
    let func = Function {
        name: Ref::Index(f),
        noend: false,
        body,
    };
    let c = crate::compile(low, &[func])?;
    Ok(c.code.into_iter().next().expect("one function").1)
}

fn hook_pieces(list: &[(Body, bool)], entries: &[u32], uber: i32) -> Vec<Piece> {
    let mut v = vec![];
    let mut e = entries.iter();
    for (body, in_uber) in list {
        if *in_uber {
            let entry = *e.next().expect("ubergraph entry");
            v.push(Piece::K(uber_call(uber, entry), None));
        } else {
            v.extend(user(&body.stmts, false));
        }
    }
    v
}

fn hook_function(low: &mut Lower, f: i32, uber: i32, h: &Hooks) -> R<Vec<KE>> {
    let orig = code_of(low.asset, f)?;
    if !matches!(orig.last(), Some(KE::ExEndOfScript(_))) {
        return Err("function does not end with EndOfScript".into());
    }
    low.set_function(f);
    let offs = offsets(low, &orig)?;
    let n = orig.len();
    let base = offs[n - 1];
    let mut blocks: Vec<Block> = vec![];
    let mut edits: Vec<(usize, usize, u32)> = vec![];
    let mut retargets: Vec<(u32, usize, u32)> = vec![];
    if !h.before.is_empty() {
        if n < 2 || low.size(&orig[0])? < 5 {
            return Err("the first statement is too small for a before hook; use replace".into());
        }
        let mut b = Block {
            pieces: hook_pieces(&h.before, &h.uber_before, uber),
            ..Default::default()
        };
        b.pieces.push(Piece::K(orig[0].clone(), Some(offs[0])));
        b.pieces.push(Piece::K(jump(offs[1]), None));
        edits.push((0, blocks.len(), 0));
        blocks.push(b);
    }
    if !h.after.is_empty() {
        let returns: Vec<usize> = (0..n)
            .filter(|&i| matches!(orig[i], KE::ExReturn(_)))
            .collect();
        if returns.is_empty() {
            return Err("no return statement for an after hook".into());
        }
        for &i in &returns {
            let mut b = Block::default();
            if low.size(&orig[i])? >= 5 {
                b.pieces = hook_pieces(&h.after, &h.uber_after, uber);
                edits.push((i, blocks.len(), 0));
            } else if i > 0 && !terminator(&orig[i - 1]) {
                let p = i - 1;
                let psize = low.size(&orig[p])?;
                if psize < 5 {
                    return Err(format!(
                        "the statement before the return at {} is too small to redirect",
                        offs[i]
                    ));
                }
                if edits.iter().any(|(x, _, _)| *x == p) {
                    return Err("the before and after hooks need the same statement".into());
                }
                b.pieces.push(Piece::K(orig[p].clone(), Some(offs[p])));
                b.pieces.extend(hook_pieces(&h.after, &h.uber_after, uber));
                edits.push((p, blocks.len(), 0));
                retargets.push((offs[i], blocks.len(), psize));
            } else {
                b.pieces = hook_pieces(&h.after, &h.uber_after, uber);
                retargets.push((offs[i], blocks.len(), 0));
            }
            b.pieces.push(Piece::K(orig[i].clone(), Some(offs[i])));
            blocks.push(b);
        }
    }
    let (mut tail, starts) = assemble(low, base, &blocks, false)?;
    let mut code: Vec<KE> = orig[..n - 1].to_vec();
    for (from, blk, extra) in &retargets {
        let to = starts[*blk] + extra;
        for e in code.iter_mut().chain(tail.iter_mut()) {
            e.walk(&mut Retarget(*from, to));
        }
    }
    edits.sort_by_key(|x| std::cmp::Reverse(x.0));
    for (i, blk, extra) in edits {
        replace_with_jump(low, &mut code, i, starts[blk] + extra)?;
    }
    if offsets(low, &code)?.last() != Some(&base) {
        return Err("internal error: the original code moved".into());
    }
    code.extend(tail);
    code.push(ke!(ExEndOfScript));
    Ok(code)
}

fn template(a: &Asset, class: i32) -> R<usize> {
    let funcs: Vec<(usize, u32, bool)> = a
        .asset_data
        .exports
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Export::FunctionExport(f)
                if e.get_base_export().outer_index.index == class
                    && f.struct_export.super_struct.index == 0 =>
            {
                Some((
                    i,
                    f.function_flags.bits(),
                    f.struct_export.loaded_properties.is_empty(),
                ))
            }
            _ => None,
        })
        .collect();
    funcs
        .iter()
        .find(|(_, flags, empty)| *flags == 0x0C00_0000 && *empty)
        .or_else(|| funcs.first())
        .map(|(i, _, _)| *i)
        .ok_or_else(|| "no function in the class to copy for a new function".to_string())
}

fn add_function(
    a: &mut Asset,
    class: i32,
    tpl: usize,
    name: &str,
    flags: u32,
    props: Vec<FProperty>,
    uber: i32,
) -> R<i32> {
    let fname = crate::names::make_fname(a, &Name::new(name));
    let mut f = a.asset_data.exports[tpl].clone();
    let idx = a.asset_data.exports.len() as i32 + 1;
    let Export::FunctionExport(fe) = &mut f else {
        return Err("template is not a function".into());
    };
    fe.function_flags = EFunctionFlags::from_bits_truncate(flags);
    let s = &mut fe.struct_export;
    s.super_struct = PackageIndex::new(0);
    s.children.clear();
    s.loaded_properties = props;
    s.script_bytecode = Some(vec![
        ke!(ExReturn {
            return_expression: Box::new(nothing())
        }),
        ke!(ExEndOfScript),
    ]);
    s.script_bytecode_raw = None;
    let b = f.get_base_export_mut();
    b.object_name = fname.clone();
    b.serialization_before_serialization_dependencies = vec![];
    b.create_before_serialization_dependencies = if uber != 0 {
        vec![PackageIndex::new(uber)]
    } else {
        vec![]
    };
    b.serialization_before_create_dependencies = vec![];
    b.create_before_create_dependencies = vec![PackageIndex::new(class)];
    a.asset_data.exports.push(f);
    let t = PackageIndex::new(tpl as i32 + 1);
    let n = PackageIndex::new(idx);
    let ce = a
        .asset_data
        .exports
        .get_mut((class - 1) as usize)
        .ok_or("class export missing")?;
    if let Export::ClassExport(c) = ce {
        c.struct_export.children.push(n);
        c.func_map.insert(fname, n);
    }
    let b = ce.get_base_export_mut();
    for v in [
        &mut b.serialization_before_serialization_dependencies,
        &mut b.create_before_serialization_dependencies,
        &mut b.serialization_before_create_dependencies,
        &mut b.create_before_create_dependencies,
    ] {
        if v.contains(&t) {
            v.push(n);
        }
    }
    Ok(idx)
}
