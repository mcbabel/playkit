use crate::ast::*;
use crate::names::{find_fname, fname_text, make_fname, Objects, Want};
use crate::{measure, Asset};
use ordered_float::OrderedFloat;
use std::collections::{BTreeSet, HashMap};
use unreal_asset::exports::{Export, ExportBaseTrait};
use unreal_asset::fproperty::FProperty;
use unreal_asset::kismet::walk::{Visitor, Walk};
use unreal_asset::kismet::*;
use unreal_asset::types::fname::FName;
use unreal_asset::types::vector::{Transform, Vector, Vector4};
use unreal_asset::types::PackageIndex;

pub type KE = KismetExpression;
pub type R<T> = std::result::Result<T, String>;

macro_rules! ke {
    ($v:ident { $($f:ident : $e:expr),* $(,)? }) => {
        KE::$v($v { token: EExprToken::$v, $($f: $e),* })
    };
    ($v:ident) => {
        KE::$v($v { token: EExprToken::$v })
    };
}
pub(crate) use ke;

pub type PKey = (i32, String);

pub fn pkey(p: &KismetPropertyPointer) -> Option<PKey> {
    let f = p.new.as_ref()?;
    if f.path.is_empty() {
        return None;
    }
    let path: Vec<String> = f.path.iter().map(fname_text).collect();
    Some((f.resolved_owner.index, path.join("/")))
}

pub fn kptr(owner: i32, path: Vec<FName>) -> KismetPropertyPointer {
    KismetPropertyPointer::from_new(FieldPath::new(path, PackageIndex::new(owner)))
}

pub fn null_ptr() -> KismetPropertyPointer {
    kptr(0, vec![])
}

pub fn var_ptr(e: &KE) -> Option<&KismetPropertyPointer> {
    match e {
        KE::ExLocalVariable(v) => Some(&v.variable),
        KE::ExInstanceVariable(v) => Some(&v.variable),
        KE::ExLocalOutVariable(v) => Some(&v.variable),
        KE::ExDefaultVariable(v) => Some(&v.variable),
        KE::ExClassSparseDataVariable(v) => Some(&v.variable),
        _ => None,
    }
}

pub fn dest_ptr(e: &KE) -> Option<&KismetPropertyPointer> {
    match e {
        KE::ExContext(c) => dest_ptr(&c.context_expression),
        KE::ExContextFailSilent(c) => dest_ptr(&c.context_expression),
        KE::ExClassContext(c) => dest_ptr(&c.context_expression),
        KE::ExStructMemberContext(c) => Some(&c.struct_member_expression),
        e => var_ptr(e),
    }
}

pub fn struct_parts(e: &Export) -> Option<&unreal_asset::exports::struct_export::StructExport> {
    match e {
        Export::ClassExport(c) => Some(&c.struct_export),
        Export::FunctionExport(f) => Some(&f.struct_export),
        Export::StructExport(s) => Some(s),
        _ => None,
    }
}

pub fn fprop_generic(p: &FProperty) -> &unreal_asset::fproperty::FGenericProperty {
    match p {
        FProperty::FGenericProperty(g) => g,
        FProperty::FEnumProperty(x) => &x.generic_property,
        FProperty::FArrayProperty(x) => &x.generic_property,
        FProperty::FSetProperty(x) => &x.generic_property,
        FProperty::FObjectProperty(x) => &x.generic_property,
        FProperty::FSoftObjectProperty(x) => &x.generic_property,
        FProperty::FClassProperty(x) => &x.generic_property,
        FProperty::FSoftClassProperty(x) => &x.generic_property,
        FProperty::FDelegateProperty(x) => &x.generic_property,
        FProperty::FMulticastDelegateProperty(x) => &x.generic_property,
        FProperty::FMulticastInlineDelegateProperty(x) => &x.generic_property,
        FProperty::FInterfaceProperty(x) => &x.generic_property,
        FProperty::FMapProperty(x) => &x.generic_property,
        FProperty::FBoolProperty(x) => &x.generic_property,
        FProperty::FByteProperty(x) => &x.generic_property,
        FProperty::FStructProperty(x) => &x.generic_property,
        FProperty::FNumericProperty(x) => &x.generic_property,
    }
}

#[derive(Default)]
pub struct Info {
    pub declared: HashMap<PKey, FProperty>,
    pub owners: HashMap<String, BTreeSet<i32>>,
    pub call_tokens: HashMap<i32, BTreeSet<CallKindKey>>,
    pub virt_tokens: HashMap<String, BTreeSet<CallKindKey>>,
    pub let_tokens: HashMap<PKey, BTreeSet<u8>>,
    pub struct_sizes: HashMap<i32, BTreeSet<i32>>,
    pub signatures: HashMap<PKey, BTreeSet<i32>>,
    pub func_flags: HashMap<i32, u32>,
}

pub type CallKindKey = u8;

fn call_key(k: CallKind) -> u8 {
    k as u8
}

fn key_call(k: u8) -> CallKind {
    match k {
        0 => CallKind::Math,
        1 => CallKind::Final,
        2 => CallKind::LocalFinal,
        3 => CallKind::Virtual,
        _ => CallKind::LocalVirtual,
    }
}

struct Collect<'a>(&'a mut Info);

impl Visitor for Collect<'_> {
    fn expr(&mut self, e: &mut KE) {
        let info = &mut *self.0;
        let mut note = |p: &KismetPropertyPointer| {
            if let Some((owner, name)) = pkey(p) {
                info.owners.entry(name).or_default().insert(owner);
            }
        };
        match &*e {
            KE::ExContext(c) => note(&c.r_value_pointer),
            KE::ExContextFailSilent(c) => note(&c.r_value_pointer),
            KE::ExClassContext(c) => note(&c.r_value_pointer),
            KE::ExStructMemberContext(c) => note(&c.struct_member_expression),
            KE::ExLet(l) => note(&l.value),
            e => {
                if let Some(p) = var_ptr(e) {
                    note(p)
                }
            }
        }
        let mut let_tok = |dest: &KE, tok: EExprToken| {
            if let Some(k) = dest_ptr(dest).and_then(pkey) {
                info.let_tokens.entry(k).or_default().insert(tok as u8);
            }
        };
        match &*e {
            KE::ExLet(l) => let_tok(&l.variable, EExprToken::ExLet),
            KE::ExLetBool(l) => let_tok(&l.variable_expression, EExprToken::ExLetBool),
            KE::ExLetObj(l) => let_tok(&l.variable_expression, EExprToken::ExLetObj),
            KE::ExLetWeakObjPtr(l) => let_tok(&l.variable_expression, EExprToken::ExLetWeakObjPtr),
            KE::ExLetDelegate(l) => let_tok(&l.variable_expression, EExprToken::ExLetDelegate),
            KE::ExLetMulticastDelegate(l) => {
                let_tok(&l.variable_expression, EExprToken::ExLetMulticastDelegate)
            }
            _ => {}
        }
        let mut call = |f: i32, k: CallKind| {
            info.call_tokens.entry(f).or_default().insert(call_key(k));
        };
        match &*e {
            KE::ExFinalFunction(c) => call(c.stack_node.index, CallKind::Final),
            KE::ExLocalFinalFunction(c) => call(c.stack_node.index, CallKind::LocalFinal),
            KE::ExCallMath(c) => call(c.stack_node.index, CallKind::Math),
            KE::ExVirtualFunction(c) => {
                info.virt_tokens
                    .entry(fname_text(&c.virtual_function_name))
                    .or_default()
                    .insert(call_key(CallKind::Virtual));
            }
            KE::ExLocalVirtualFunction(c) => {
                info.virt_tokens
                    .entry(fname_text(&c.virtual_function_name))
                    .or_default()
                    .insert(call_key(CallKind::LocalVirtual));
            }
            KE::ExStructConst(s) => {
                info.struct_sizes
                    .entry(s.struct_value.index)
                    .or_default()
                    .insert(s.struct_size);
            }
            KE::ExCallMulticastDelegate(c) => {
                if let Some(k) = dest_ptr(&c.delegate).and_then(pkey) {
                    info.signatures
                        .entry(k)
                        .or_default()
                        .insert(c.stack_node.index);
                }
            }
            _ => {}
        }
    }
}

impl Info {
    pub fn new(a: &Asset) -> Self {
        let mut info = Info::default();
        for (i, e) in a.asset_data.exports.iter().enumerate() {
            let idx = i as i32 + 1;
            if let Export::FunctionExport(f) = e {
                info.func_flags.insert(idx, f.function_flags.bits());
            }
            let Some(s) = struct_parts(e) else { continue };
            for p in &s.loaded_properties {
                let name = fname_text(&fprop_generic(p).name);
                info.declared.insert((idx, name), p.clone());
            }
            if let Some(code) = &s.script_bytecode {
                for x in code {
                    x.clone().walk(&mut Collect(&mut info));
                }
            }
        }
        for (owner, name) in info.declared.keys() {
            info.owners.entry(name.clone()).or_default().insert(*owner);
        }
        info
    }
}

#[derive(Clone, Copy, Default)]
pub struct Env<'p> {
    pub member: bool,
    pub owner_hint: i32,
    pub rv: Option<&'p KismetPropertyPointer>,
    pub uber_arg: bool,
    pub pos: u32,
}

impl Env<'_> {
    pub fn at(pos: u32) -> Env<'static> {
        Env {
            pos,
            ..Default::default()
        }
    }
}

pub struct Lower<'a> {
    pub asset: &'a Asset,
    pub objs: Objects,
    pub info: Info,
    pub create: bool,
    pub func: i32,
    pub class: i32,
    pub uber: i32,
    pub labels: HashMap<String, u32>,
    pub uber_labels: HashMap<String, u32>,
    pub dummy: bool,
    pub aliases: HashMap<String, String>,
}

fn f64of(b: u32) -> OrderedFloat<f64> {
    OrderedFloat(f32::from_bits(b) as f64)
}

pub fn builtin_struct_size(name: &str) -> Option<i32> {
    Some(match name {
        "Vector" | "Rotator" => 12,
        "Vector2D" | "IntPoint" | "SteamID" => 8,
        "Vector4" | "Quat" | "LinearColor" | "Margin" | "Guid" | "IntVector4" => 16,
        "Color" => 4,
        "Transform" => 48,
        "LatentActionInfo" => 24,
        "Timespan" | "DateTime" => 8,
        _ => return None,
    })
}

pub fn token_by_name(t: &str) -> Option<KE> {
    Some(match t {
        "EndParmValue" => ke!(ExEndParmValue),
        "EndFunctionParms" => ke!(ExEndFunctionParms),
        "EndStructConst" => ke!(ExEndStructConst),
        "EndArray" => ke!(ExEndArray),
        "EndArrayConst" => ke!(ExEndArrayConst),
        "EndSet" => ke!(ExEndSet),
        "EndSetConst" => ke!(ExEndSetConst),
        "EndMap" => ke!(ExEndMap),
        "EndMapConst" => ke!(ExEndMapConst),
        "Breakpoint" => ke!(ExBreakpoint),
        "Tracepoint" => ke!(ExTracepoint),
        "WireTracepoint" => ke!(ExWireTracepoint),
        "DeprecatedOp4A" => ke!(ExDeprecatedOp4A),
        "PopExecutionFlow" => ke!(ExPopExecutionFlow),
        "EndOfScript" => ke!(ExEndOfScript),
        "Nothing" => ke!(ExNothing),
        _ => return None,
    })
}

pub fn instr_by_name(t: &str) -> Option<EScriptInstrumentationType> {
    use EScriptInstrumentationType as I;
    Some(match t {
        "Class" => I::Class,
        "ClassScope" => I::ClassScope,
        "Instance" => I::Instance,
        "Event" => I::Event,
        "InlineEvent" => I::InlineEvent,
        "ResumeEvent" => I::ResumeEvent,
        "PureNodeEntry" => I::PureNodeEntry,
        "NodeDebugSite" => I::NodeDebugSite,
        "NodeEntry" => I::NodeEntry,
        "NodeExit" => I::NodeExit,
        "PushState" => I::PushState,
        "RestoreState" => I::RestoreState,
        "ResetState" => I::ResetState,
        "SuspendState" => I::SuspendState,
        "PopState" => I::PopState,
        "TunnelEndOfThread" => I::TunnelEndOfThread,
        "Stop" => I::Stop,
        _ => return None,
    })
}

fn let_kw_of(tok: u8) -> LetKw {
    match tok {
        x if x == EExprToken::ExLetBool as u8 => LetKw::Bool,
        x if x == EExprToken::ExLetObj as u8 => LetKw::Obj,
        x if x == EExprToken::ExLetWeakObjPtr as u8 => LetKw::Weak,
        x if x == EExprToken::ExLetDelegate as u8 => LetKw::Delegate,
        x if x == EExprToken::ExLetMulticastDelegate as u8 => LetKw::Multicast,
        _ => LetKw::Let,
    }
}

fn let_kw_of_prop(p: &FProperty) -> LetKw {
    match p {
        FProperty::FBoolProperty(_) => LetKw::Bool,
        FProperty::FObjectProperty(_) | FProperty::FClassProperty(_) => LetKw::Obj,
        FProperty::FDelegateProperty(_) => LetKw::Delegate,
        FProperty::FMulticastDelegateProperty(_)
        | FProperty::FMulticastInlineDelegateProperty(_) => LetKw::Multicast,
        FProperty::FGenericProperty(g)
            if g.serialized_type
                .as_ref()
                .is_some_and(|t| t.get_content() == "WeakObjectProperty") =>
        {
            LetKw::Weak
        }
        _ => LetKw::Let,
    }
}

const OUT_PARM: u64 = 0x100;

impl<'a> Lower<'a> {
    pub fn new(asset: &'a Asset, create: bool) -> Self {
        Lower {
            asset,
            objs: Objects::new(asset),
            info: Info::new(asset),
            create,
            func: 0,
            class: 0,
            uber: 0,
            labels: HashMap::new(),
            uber_labels: HashMap::new(),
            dummy: false,
            aliases: HashMap::new(),
        }
    }

    pub fn set_function(&mut self, func: i32) {
        self.func = func;
        self.class = self.objs.get(func).map(|o| o.outer).unwrap_or(0);
        self.uber = self
            .asset
            .asset_data
            .exports
            .iter()
            .enumerate()
            .find(|(_, e)| {
                let b = e.get_base_export();
                b.outer_index.index == self.class
                    && b.object_name.get_content().starts_with("ExecuteUbergraph_")
                    && matches!(e, Export::FunctionExport(_))
            })
            .map(|(i, _)| i as i32 + 1)
            .unwrap_or(0);
    }

    pub fn size(&self, e: &KE) -> R<u32> {
        measure(self.asset, e).map(|s| s as u32)
    }

    pub fn fname(&mut self, n: &Name) -> R<FName> {
        if let Some(f) = find_fname(self.asset, n) {
            return Ok(f);
        }
        if self.create {
            return Ok(make_fname(self.asset, n));
        }
        Err(format!("name {} is not in the asset", n.text))
    }

    pub fn object(&mut self, r: &Ref, want: Want) -> R<i32> {
        match self.objs.resolve(r, want).as_slice() {
            [i] => Ok(*i),
            [] if self.create => self.objs.create(self.asset, r, want),
            _ => self.objs.resolve_one(r, want),
        }
    }

    fn declared(&self, owner: i32, name: &str) -> Option<&FProperty> {
        self.info.declared.get(&(owner, name.to_string()))
    }

    fn unique_owner(&self, name: &str, structs_only: bool) -> Option<i32> {
        let set = self.info.owners.get(name)?;
        let mut it = set.iter().copied().filter(|&o| {
            o != 0 && !self.objs.is_function(o) && (!structs_only || self.objs.is_struct(o))
        });
        let first = it.next()?;
        if it.next().is_some() {
            return None;
        }
        Some(first)
    }

    fn path_fnames(&mut self, path: &[Name]) -> R<Vec<FName>> {
        path.iter().map(|n| self.fname(n)).collect()
    }

    pub fn explicit_ptr(&mut self, p: &Ptr) -> R<KismetPropertyPointer> {
        let owner = match &p.owner {
            Some(r) => self.object(r, Want::Any)?,
            None => {
                let name = p.path.first().map(|n| n.text.clone()).unwrap_or_default();
                if p.path.len() == 1 {
                    self.unique_owner(&name, false).ok_or_else(|| {
                        format!(
                            "cannot tell which class or struct owns {name}; write Owner::{name}"
                        )
                    })?
                } else {
                    return Err("a property path needs an owner".into());
                }
            }
        };
        let path = self.path_fnames(&p.path)?;
        Ok(kptr(owner, path))
    }

    fn var_kind_for(&self, owner: i32, name: &str) -> VarKind {
        if owner == self.func {
            match self.declared(owner, name) {
                Some(p) if fprop_generic(p).property_flags.bits() & OUT_PARM != 0 => VarKind::Out,
                _ => VarKind::Local,
            }
        } else {
            VarKind::Inst
        }
    }

    fn resolve_bare(&self, name: &str, member: bool) -> Option<(VarKind, i32)> {
        if member {
            return self.unique_owner(name, false).map(|o| (VarKind::Inst, o));
        }
        if self.declared(self.func, name).is_some() {
            return Some((self.var_kind_for(self.func, name), self.func));
        }
        if self.declared(self.class, name).is_some() {
            return Some((VarKind::Inst, self.class));
        }
        self.unique_owner(name, false).map(|o| (VarKind::Inst, o))
    }

    fn var_expr(kind: VarKind, p: KismetPropertyPointer) -> KE {
        match kind {
            VarKind::Local => ke!(ExLocalVariable { variable: p }),
            VarKind::Out => ke!(ExLocalOutVariable { variable: p }),
            VarKind::Inst => ke!(ExInstanceVariable { variable: p }),
            VarKind::Default => ke!(ExDefaultVariable { variable: p }),
            VarKind::Sparse => ke!(ExClassSparseDataVariable { variable: p }),
        }
    }

    fn lower_var(&mut self, p: &Ptr, env: Env) -> R<KE> {
        match &p.owner {
            None if p.path.len() == 1 => {
                let mut name = p.path[0].clone();
                if !env.member {
                    if let Some(a) = self.aliases.get(&name.text) {
                        name = Name::new(a.clone());
                    }
                }
                let found = match self.resolve_bare(&name.text, env.member) {
                    None if env.member && env.owner_hint != 0 => {
                        Some((VarKind::Inst, env.owner_hint))
                    }
                    r => r,
                };
                if let Some((kind, owner)) = found {
                    let f = self.fname(&name)?;
                    return Ok(Self::var_expr(kind, kptr(owner, vec![f])));
                }
                if !env.member {
                    let r = Ref::Path(vec![name.clone()]);
                    if let [i] = self.objs.resolve(&r, Want::Any).as_slice() {
                        return Ok(ke!(ExObjectConst {
                            value: PackageIndex::new(*i)
                        }));
                    }
                }
                Err(format!(
                    "unknown variable {}; write Owner::{} for a variable of another class",
                    name.text, name.text
                ))
            }
            _ => {
                let ptr = self.explicit_ptr(p)?;
                let owner = ptr
                    .new
                    .as_ref()
                    .map(|f| f.resolved_owner.index)
                    .unwrap_or(0);
                let name = p.path.first().map(|n| n.text.clone()).unwrap_or_default();
                let kind = if env.member {
                    VarKind::Inst
                } else {
                    self.var_kind_for(owner, &name)
                };
                Ok(Self::var_expr(kind, ptr))
            }
        }
    }

    fn kind_var(&mut self, kind: VarKind, p: &Ptr) -> R<KE> {
        let ptr = match (&p.owner, kind) {
            (None, VarKind::Local | VarKind::Out) if p.path.len() == 1 => {
                let f = self.fname(&p.path[0])?;
                kptr(self.func, vec![f])
            }
            (None, _)
                if p.path.len() == 1 && self.declared(self.class, &p.path[0].text).is_some() =>
            {
                let f = self.fname(&p.path[0])?;
                kptr(self.class, vec![f])
            }
            _ => self.explicit_ptr(p)?,
        };
        Ok(Self::var_expr(kind, ptr))
    }

    fn target(&self, t: &Target, uber: bool) -> R<u32> {
        match t {
            Target::Raw(n) => Ok(*n),
            Target::Label(_) if self.dummy => Ok(0),
            Target::Label(l) => {
                let map = if uber {
                    &self.uber_labels
                } else {
                    &self.labels
                };
                map.get(l)
                    .copied()
                    .ok_or_else(|| format!("unknown label {l}"))
            }
        }
    }

    fn seq(&mut self, items: &[Expr], mut pos: u32) -> R<(Vec<KE>, u32)> {
        let mut out = Vec::with_capacity(items.len());
        for x in items {
            let e = self.expr(x, Env::at(pos))?;
            pos += self.size(&e)?;
            out.push(e);
        }
        Ok((out, pos))
    }

    pub fn infer_call(&self, f: Option<i32>, name: &str, member: bool) -> CallKind {
        let unique = |s: Option<&BTreeSet<u8>>| match s {
            Some(s) if s.len() == 1 => s.iter().next().map(|k| key_call(*k)),
            _ => None,
        };
        if let Some(f) = f {
            if let Some(k) = unique(self.info.call_tokens.get(&f)) {
                return k;
            }
            if let Some(&flags) = self.info.func_flags.get(&f) {
                let fin = flags & 0x1 != 0;
                let native = flags & 0x400 != 0;
                let net = flags & (0x40 | 0x80 | 0x4000 | 0x200000 | 0x1000000) != 0;
                let math = fin && flags & 0x10002400 == 0x10002400 && flags & 0xc == 0;
                let local = !native && !net;
                return match (math, fin, local) {
                    (true, _, _) => CallKind::Math,
                    (_, true, true) => CallKind::LocalFinal,
                    (_, true, false) => CallKind::Final,
                    (_, false, true) => CallKind::LocalVirtual,
                    (_, false, false) => CallKind::Virtual,
                };
            }
            if let Some(k) = unique(self.info.virt_tokens.get(name)) {
                return k;
            }
            if self.objs.is_native(f) {
                return if member {
                    CallKind::Final
                } else {
                    CallKind::Math
                };
            }
            return CallKind::LocalVirtual;
        }
        unique(self.info.virt_tokens.get(name)).unwrap_or(CallKind::Virtual)
    }

    fn call(&mut self, kind: Option<CallKind>, head: &Ref, args: &[Expr], env: Env) -> R<KE> {
        let single = match head {
            Ref::Path(s) if s.len() == 1 => Some(s[0].clone()),
            _ => None,
        };
        let virt = matches!(kind, Some(CallKind::Virtual | CallKind::LocalVirtual));
        let func = if virt {
            None
        } else {
            let found: Vec<i32> = self
                .objs
                .resolve(head, Want::Function)
                .into_iter()
                .filter(|&i| self.objs.is_function(i))
                .collect();
            match found.as_slice() {
                [f] => Some(*f),
                [] if matches!(head, Ref::Quoted { .. } | Ref::Index(_)) || kind.is_some() => {
                    Some(self.object(head, Want::Function)?)
                }
                [] => None,
                _ => return Err(self.objs.resolve_one(head, Want::Function).unwrap_err()),
            }
        };
        let name = match (func, &single) {
            (Some(f), _) => Name::new(self.objs.name(f)),
            (None, Some(n)) => n.clone(),
            (None, None) => return Err(format!("function {head} not found")),
        };
        let kind = match kind {
            Some(k) => k,
            None => self.infer_call(func, &name.text, env.member),
        };
        let uber = func.is_some_and(|f| f == self.uber && f != 0);
        match kind {
            CallKind::Virtual | CallKind::LocalVirtual => {
                let fname = self.fname(&name)?;
                let mut pos = env.pos + 13;
                let mut params = vec![];
                for a in args {
                    let e = self.expr(a, Env::at(pos))?;
                    pos += self.size(&e)?;
                    params.push(e);
                }
                Ok(if kind == CallKind::Virtual {
                    ke!(ExVirtualFunction {
                        virtual_function_name: fname,
                        parameters: params
                    })
                } else {
                    ke!(ExLocalVirtualFunction {
                        virtual_function_name: fname,
                        parameters: params
                    })
                })
            }
            _ => {
                let f = match func {
                    Some(f) => f,
                    None => self.object(head, Want::Function)?,
                };
                let mut pos = env.pos + 9;
                let mut params = vec![];
                for a in args {
                    let e = self.expr(
                        a,
                        Env {
                            uber_arg: uber,
                            ..Env::at(pos)
                        },
                    )?;
                    pos += self.size(&e)?;
                    params.push(e);
                }
                let stack_node = PackageIndex::new(f);
                Ok(match kind {
                    CallKind::Math => ke!(ExCallMath {
                        stack_node: stack_node,
                        parameters: params
                    }),
                    CallKind::Final => ke!(ExFinalFunction {
                        stack_node: stack_node,
                        parameters: params
                    }),
                    _ => ke!(ExLocalFinalFunction {
                        stack_node: stack_node,
                        parameters: params
                    }),
                })
            }
        }
    }

    pub fn struct_size(&self, ty: i32) -> Option<i32> {
        if let Some(s) = self.info.struct_sizes.get(&ty) {
            if s.len() == 1 {
                return s.iter().next().copied();
            }
        }
        builtin_struct_size(&self.objs.name(ty))
    }

    fn lit(&mut self, l: &Lit) -> R<KE> {
        Ok(match l {
            Lit::Int(v) => ke!(ExIntConst { value: *v }),
            Lit::Int64(v) => ke!(ExInt64Const { value: *v }),
            Lit::UInt64(v) => ke!(ExUInt64Const { value: *v }),
            Lit::Byte(v) => ke!(ExByteConst { value: *v }),
            Lit::IntByte(v) => ke!(ExIntConstByte { value: *v }),
            Lit::IntZero => ke!(ExIntZero),
            Lit::IntOne => ke!(ExIntOne),
            Lit::Float(b) => ke!(ExFloatConst {
                value: OrderedFloat(f32::from_bits(*b))
            }),
            Lit::True => ke!(ExTrue),
            Lit::False => ke!(ExFalse),
            Lit::SelfObj => ke!(ExSelf),
            Lit::NoObject => ke!(ExNoObject),
            Lit::NoInterface => ke!(ExNoInterface),
            Lit::Nothing => ke!(ExNothing),
            Lit::Str(s) if s.is_ascii() => ke!(ExStringConst { value: s.clone() }),
            Lit::Str(s) | Lit::UniStr(s) => ke!(ExUnicodeStringConst { value: s.clone() }),
            Lit::AnsiStr(s) => ke!(ExStringConst { value: s.clone() }),
            Lit::Name(n) => {
                let value = self.fname(n)?;
                ke!(ExNameConst { value: value })
            }
            Lit::Vector(v) => ke!(ExVectorConst {
                value: Vector::new(f64of(v[0]), f64of(v[1]), f64of(v[2]))
            }),
            Lit::Rotator(v) => ke!(ExRotationConst {
                rotator: Vector::new(f64of(v[0]), f64of(v[1]), f64of(v[2]))
            }),
            Lit::Transform(v) => ke!(ExTransformConst {
                value: Transform::new(
                    Vector4::new(f64of(v[0]), f64of(v[1]), f64of(v[2]), f64of(v[3])),
                    Vector::new(f64of(v[4]), f64of(v[5]), f64of(v[6])),
                    Vector::new(f64of(v[7]), f64of(v[8]), f64of(v[9])),
                )
            }),
            Lit::Token(t) => token_by_name(t).ok_or_else(|| format!("unknown token {t}"))?,
        })
    }

    fn text(&mut self, t: &Text, pos: u32) -> R<KE> {
        let mut st = FScriptText {
            text_literal_type: EBlueprintTextLiteralType::Empty,
            localized_source: None,
            localized_key: None,
            localized_namespace: None,
            invariant_literal_string: None,
            literal_string: None,
            string_table_asset: None,
            string_table_id: None,
            string_table_key: None,
        };
        let p = pos + 2;
        match t {
            Text::Empty => {}
            Text::Localized(ns, key, src) => {
                st.text_literal_type = EBlueprintTextLiteralType::LocalizedText;
                let (v, _) = self.seq(&[src.clone(), key.clone(), ns.clone()], p)?;
                let mut it = v.into_iter();
                st.localized_source = it.next();
                st.localized_key = it.next();
                st.localized_namespace = it.next();
            }
            Text::Invariant(e) => {
                st.text_literal_type = EBlueprintTextLiteralType::InvariantText;
                st.invariant_literal_string = Some(self.expr(e, Env::at(p))?);
            }
            Text::Literal(e) => {
                st.text_literal_type = EBlueprintTextLiteralType::LiteralString;
                st.literal_string = Some(self.expr(e, Env::at(p))?);
            }
            Text::Table(r, id, key) => {
                st.text_literal_type = EBlueprintTextLiteralType::StringTableEntry;
                st.string_table_asset = Some(PackageIndex::new(self.object(r, Want::Any)?));
                let (v, _) = self.seq(&[id.clone(), key.clone()], p + 8)?;
                let mut it = v.into_iter();
                st.string_table_id = it.next();
                st.string_table_key = it.next();
            }
        }
        Ok(ke!(ExTextConst {
            value: Box::new(st)
        }))
    }

    pub fn expr(&mut self, e: &Expr, env: Env) -> R<KE> {
        let pos = env.pos;
        let b = Box::new;
        Ok(match e {
            Expr::Var(p) => self.lower_var(p, env)?,
            Expr::KindVar(k, p) => self.kind_var(*k, p)?,
            Expr::Obj(r) => ke!(ExObjectConst {
                value: PackageIndex::new(self.object(r, Want::Any)?)
            }),
            Expr::Lit(l) => self.lit(l)?,
            Expr::Call { kind, head, args } => self.call(*kind, head, args, env)?,
            Expr::Ctx {
                obj,
                op,
                rv,
                skip,
                inner,
            } => self.ctx(obj, *op, rv.as_ref(), *skip, inner, env)?,
            Expr::Member(x, p) => {
                let m = match (&p.owner, p.path.as_slice()) {
                    (None, [n]) => {
                        let owner = self.unique_owner(&n.text, true).ok_or_else(|| {
                            format!(
                                "cannot tell which struct owns {}; write Struct::{}",
                                n.text, n.text
                            )
                        })?;
                        let f = self.fname(n)?;
                        kptr(owner, vec![f])
                    }
                    _ => self.explicit_ptr(p)?,
                };
                let s = self.expr(x, Env::at(pos + 9))?;
                ke!(ExStructMemberContext {
                    struct_member_expression: m,
                    struct_expression: b(s)
                })
            }
            Expr::Index(a, i) => {
                let av = self.expr(a, Env::at(pos + 1))?;
                let p2 = pos + 1 + self.size(&av)?;
                let iv = self.expr(i, Env::at(p2))?;
                ke!(ExArrayGetByRef {
                    array_variable: b(av),
                    array_index: b(iv)
                })
            }
            Expr::Cast(kind, c, x) => {
                let class = PackageIndex::new(self.object(c, Want::Class)?);
                let t = b(self.expr(x, Env::at(pos + 9))?);
                match kind {
                    CastKind::Dynamic => ke!(ExDynamicCast {
                        class_ptr: class,
                        target_expression: t
                    }),
                    CastKind::Meta => ke!(ExMetaCast {
                        class_ptr: class,
                        target_expression: t
                    }),
                    CastKind::ObjToIface => ke!(ExObjToInterfaceCast {
                        class_ptr: class,
                        target: t
                    }),
                    CastKind::CrossIface => ke!(ExCrossInterfaceCast {
                        class_ptr: class,
                        target: t
                    }),
                    CastKind::IfaceToObj => ke!(ExInterfaceToObjCast {
                        class_ptr: class,
                        target: t
                    }),
                }
            }
            Expr::PrimCast(t, x) => {
                let conversion_type = match t.as_str() {
                    "ObjectToInterface" => ECastToken::ObjectToInterface,
                    "ObjectToBool" => ECastToken::ObjectToBool,
                    "InterfaceToBool" => ECastToken::InterfaceToBool,
                    "Max" => ECastToken::Max,
                    _ => return Err(format!("unknown primitive cast {t}")),
                };
                let target = b(self.expr(x, Env::at(pos + 2))?);
                ke!(ExPrimitiveCast {
                    conversion_type: conversion_type,
                    target: target
                })
            }
            Expr::Struct { ty, size, fields } => {
                let t = self.object(ty, Want::Struct)?;
                let size = match size {
                    Some(s) => *s,
                    None => self.struct_size(t).ok_or_else(|| {
                        format!("size of struct {ty} unknown; write {ty}<size>{{...}}")
                    })?,
                };
                let (value, _) = self.seq(fields, pos + 13)?;
                ke!(ExStructConst {
                    struct_value: PackageIndex::new(t),
                    struct_size: size,
                    value: value
                })
            }
            Expr::Switch {
                index,
                cases,
                default,
            } => {
                let mut p = pos + 7;
                let idx = self.expr(index, Env::at(p))?;
                p += self.size(&idx)?;
                let mut out = vec![];
                for (c, v) in cases {
                    let ce = self.expr(c, Env::at(p))?;
                    p += self.size(&ce)? + 4;
                    let ve = self.expr(v, Env::at(p))?;
                    p += self.size(&ve)?;
                    out.push(KismetSwitchCase::new(ce, p, ve));
                }
                let d = self.expr(default, Env::at(p))?;
                p += self.size(&d)?;
                ke!(ExSwitchValue {
                    end_goto_offset: p,
                    index_term: b(idx),
                    default_term: b(d),
                    cases: out
                })
            }
            Expr::Addr(t) => {
                if env.uber_arg {
                    ke!(ExIntConst {
                        value: self.target(t, true)? as i32
                    })
                } else {
                    ke!(ExSkipOffsetConst {
                        value: self.target(t, false)?
                    })
                }
            }
            Expr::SkipOffset(t) => ke!(ExSkipOffsetConst {
                value: self.target(t, false)?
            }),
            Expr::Text(t) => self.text(t, pos)?,
            Expr::Bind(d, o, n) => {
                let function_name = self.fname(n)?;
                let dv = self.expr(d, Env::at(pos + 13))?;
                let p2 = pos + 13 + self.size(&dv)?;
                let ov = self.expr(o, Env::at(p2))?;
                ke!(ExBindDelegate {
                    function_name: function_name,
                    delegate: b(dv),
                    object_term: b(ov)
                })
            }
            Expr::AddMulti(x, y) | Expr::RemoveMulti(x, y) => {
                let xv = self.expr(x, Env::at(pos + 1))?;
                let p2 = pos + 1 + self.size(&xv)?;
                let yv = self.expr(y, Env::at(p2))?;
                if matches!(e, Expr::AddMulti(..)) {
                    ke!(ExAddMulticastDelegate {
                        delegate: b(xv),
                        delegate_to_add: b(yv)
                    })
                } else {
                    ke!(ExRemoveMulticastDelegate {
                        delegate: b(xv),
                        delegate_to_add: b(yv)
                    })
                }
            }
            Expr::ClearMulti(x) => ke!(ExClearMulticastDelegate {
                delegate_to_clear: b(self.expr(x, Env::at(pos + 1))?)
            }),
            Expr::Broadcast { sig, del, args } => {
                let dv = self.expr(del, Env::at(pos + 9))?;
                let sig = match sig {
                    Some(r) => self.object(r, Want::Function)?,
                    None => self.infer_signature(&dv).ok_or_else(|| {
                        "cannot tell the delegate signature; write broadcast<Signature>(...)"
                            .to_string()
                    })?,
                };
                let p2 = pos + 9 + self.size(&dv)?;
                let (params, _) = self.seq(args, p2)?;
                ke!(ExCallMulticastDelegate {
                    stack_node: PackageIndex::new(sig),
                    parameters: params,
                    delegate: b(dv)
                })
            }
            Expr::Assert(x, line, dbg) => ke!(ExAssert {
                line_number: *line,
                debug_mode: *dbg,
                assert_expression: b(self.expr(x, Env::at(pos + 4))?)
            }),
            Expr::Instrument(t, n) => {
                let event_type =
                    instr_by_name(t).ok_or_else(|| format!("unknown instrumentation {t}"))?;
                let event_name = match n {
                    Some(n) => Some(self.fname(n)?),
                    None => None,
                };
                ke!(ExInstrumentationEvent {
                    event_type: event_type,
                    event_name: event_name
                })
            }
            Expr::FieldPath(x) => ke!(ExFieldPathConst {
                value: b(self.expr(x, Env::at(pos + 1))?)
            }),
            Expr::Property(p) => ke!(ExPropertyConst {
                property: self.explicit_ptr(p)?
            }),
            Expr::SoftObj(x) => ke!(ExSoftObjectConst {
                value: b(self.expr(x, Env::at(pos + 1))?)
            }),
            Expr::Delegate(n) => ke!(ExInstanceDelegate {
                function_name: self.fname(n)?
            }),
            Expr::Interface(x) => ke!(ExInterfaceContext {
                interface_value: b(self.expr(x, Env::at(pos + 1))?)
            }),
            Expr::Skip(x) => {
                let s = self.expr(x, Env::at(pos + 5))?;
                ke!(ExSkip {
                    code_offset: self.size(&s)?,
                    skip_expression: b(s)
                })
            }
            Expr::ArrayConst(p, v) => {
                let inner_property = self.explicit_ptr(p)?;
                let (elements, _) = self.seq(v, pos + 13)?;
                ke!(ExArrayConst {
                    inner_property: inner_property,
                    elements: elements
                })
            }
            Expr::SetConst(p, v) => {
                let inner_property = self.explicit_ptr(p)?;
                let (elements, _) = self.seq(v, pos + 13)?;
                ke!(ExSetConst {
                    inner_property: inner_property,
                    elements: elements
                })
            }
            Expr::MapConst(k, v, items) => {
                let key_property = self.explicit_ptr(k)?;
                let value_property = self.explicit_ptr(v)?;
                let flat: Vec<Expr> = items
                    .iter()
                    .flat_map(|(a, b)| [a.clone(), b.clone()])
                    .collect();
                let (elements, _) = self.seq(&flat, pos + 21)?;
                ke!(ExMapConst {
                    key_property: key_property,
                    value_property: value_property,
                    elements: elements
                })
            }
            Expr::Stmt(s) => self
                .stmt(s, pos)?
                .ok_or_else(|| "a label cannot be used as an expression".to_string())?,
        })
    }

    pub fn static_type(&self, e: &KE) -> i32 {
        match e {
            KE::ExDynamicCast(c) => c.class_ptr.index,
            KE::ExSelf(_) => self.class,
            KE::ExContext(c) => self.static_type(&c.context_expression),
            KE::ExStructMemberContext(m) => self.prop_type(&m.struct_member_expression),
            e => var_ptr(e).map(|p| self.prop_type(p)).unwrap_or(0),
        }
    }

    fn prop_type(&self, p: &KismetPropertyPointer) -> i32 {
        let Some(k) = pkey(p) else { return 0 };
        match self.info.declared.get(&k) {
            Some(FProperty::FStructProperty(s)) => s.struct_value.index,
            Some(FProperty::FObjectProperty(o)) => o.property_class.index,
            Some(FProperty::FInterfaceProperty(o)) => o.interface_class.index,
            _ => 0,
        }
    }

    fn infer_signature(&self, del: &KE) -> Option<i32> {
        let k = dest_ptr(del).and_then(pkey)?;
        if let Some(s) = self.info.signatures.get(&k) {
            if s.len() == 1 {
                return s.iter().next().copied();
            }
        }
        match self.info.declared.get(&k)? {
            FProperty::FMulticastDelegateProperty(d) => Some(d.signature_function.index),
            FProperty::FMulticastInlineDelegateProperty(d) => Some(d.signature_function.index),
            FProperty::FDelegateProperty(d) => Some(d.signature_function.index),
            _ => None,
        }
    }

    fn ctx(
        &mut self,
        obj: &Expr,
        op: CtxOp,
        rv: Option<&Ptr>,
        skip: Option<u32>,
        inner: &Expr,
        env: Env,
    ) -> R<KE> {
        let pos = env.pos;
        let o = self.expr(obj, Env::at(pos + 1))?;
        let ipos = pos + 1 + self.size(&o)? + 12;
        let i = self.expr(
            inner,
            Env {
                member: true,
                owner_hint: self.static_type(&o),
                ..Env::at(ipos)
            },
        )?;
        if op == CtxOp::Normal && rv.is_none() && skip.is_none() && matches!(inner, Expr::Var(_)) {
            if let KE::ExInstanceVariable(v) = &i {
                let owner = v.variable.new.as_ref().map(|f| f.resolved_owner.index);
                if owner.is_some_and(|o| self.objs.is_struct(o)) {
                    let s = self.expr(obj, Env::at(pos + 9))?;
                    return Ok(ke!(ExStructMemberContext {
                        struct_member_expression: v.variable.clone(),
                        struct_expression: Box::new(s)
                    }));
                }
            }
        }
        let r_value_pointer = match rv {
            Some(p) => self.explicit_ptr(p)?,
            None => match var_ptr(&i) {
                Some(p) => p.clone(),
                None => env.rv.cloned().unwrap_or_else(null_ptr),
            },
        };
        let offset = match skip {
            Some(s) => s,
            None => self.size(&i)?,
        };
        let (o, i) = (Box::new(o), Box::new(i));
        Ok(match op {
            CtxOp::Normal => ke!(ExContext {
                object_expression: o,
                offset: offset,
                r_value_pointer: r_value_pointer,
                context_expression: i
            }),
            CtxOp::FailSilent => ke!(ExContextFailSilent {
                object_expression: o,
                offset: offset,
                r_value_pointer: r_value_pointer,
                context_expression: i
            }),
            CtxOp::Class => ke!(ExClassContext {
                object_expression: o,
                offset: offset,
                r_value_pointer: r_value_pointer,
                context_expression: i
            }),
        })
    }

    pub fn infer_let(&self, dest: &KE) -> LetKw {
        let Some(k) = dest_ptr(dest).and_then(pkey) else {
            return LetKw::Let;
        };
        if let Some(s) = self.info.let_tokens.get(&k) {
            if s.len() == 1 {
                return let_kw_of(*s.iter().next().expect("one"));
            }
        }
        match self.info.declared.get(&k) {
            Some(p) => let_kw_of_prop(p),
            None => LetKw::Let,
        }
    }

    fn frame_dest(&mut self, dest: &Expr) -> R<Option<KismetPropertyPointer>> {
        let Expr::Var(p) = dest else { return Ok(None) };
        if p.path.len() != 1 || self.uber == 0 || self.func == self.uber {
            return Ok(None);
        }
        match &p.owner {
            None => {
                let n = &match self.aliases.get(&p.path[0].text) {
                    Some(a) => Name::new(a.clone()),
                    None => p.path[0].clone(),
                };
                if self.resolve_bare(&n.text, false).is_some()
                    || self.declared(self.uber, &n.text).is_none()
                {
                    return Ok(None);
                }
                let f = self.fname(n)?;
                Ok(Some(kptr(self.uber, vec![f])))
            }
            Some(_) => {
                let ptr = self.explicit_ptr(p)?;
                let owner = ptr
                    .new
                    .as_ref()
                    .map(|f| f.resolved_owner.index)
                    .unwrap_or(0);
                if owner > 0 && owner != self.func && self.objs.is_function(owner) {
                    Ok(Some(ptr))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn let_stmt(
        &mut self,
        kw: Option<LetKw>,
        ptr: Option<&Ptr>,
        dest: &Expr,
        value: &Expr,
        pos: u32,
    ) -> R<KE> {
        let frame = match kw {
            Some(LetKw::Frame) => Some(match dest {
                Expr::Var(p) if p.owner.is_none() && p.path.len() == 1 && self.uber != 0 => {
                    let f = self.fname(&p.path[0])?;
                    kptr(self.uber, vec![f])
                }
                Expr::Var(p) | Expr::KindVar(_, p) => self.explicit_ptr(p)?,
                _ => return Err("letframe needs a variable".into()),
            }),
            None => self.frame_dest(dest)?,
            _ => None,
        };
        if let Some(p) = frame {
            let v = self.expr(
                value,
                Env {
                    rv: Some(&p),
                    ..Env::at(pos + 9)
                },
            )?;
            return Ok(ke!(ExLetValueOnPersistentFrame {
                destination_property: p,
                assignment_expression: Box::new(v)
            }));
        }
        let d1 = self.expr(dest, Env::at(pos + 1))?;
        let kw = match kw {
            Some(k) => k,
            None if ptr.is_some() => LetKw::Let,
            None => self.infer_let(&d1),
        };
        if kw == LetKw::Let {
            let d = self.expr(dest, Env::at(pos + 9))?;
            let cast = matches!(value, Expr::Cast(..) | Expr::PrimCast(..));
            let p = match ptr {
                Some(p) => self.explicit_ptr(p)?,
                None if cast => null_ptr(),
                None => dest_ptr(&d).cloned().unwrap_or_else(null_ptr),
            };
            let vpos = pos + 9 + self.size(&d)?;
            let v = self.expr(
                value,
                Env {
                    rv: Some(&p),
                    ..Env::at(vpos)
                },
            )?;
            return Ok(ke!(ExLet {
                value: p,
                variable: Box::new(d),
                expression: Box::new(v)
            }));
        }
        let rvp = dest_ptr(&d1).cloned();
        let vpos = pos + 1 + self.size(&d1)?;
        let v = self.expr(
            value,
            Env {
                rv: rvp.as_ref(),
                ..Env::at(vpos)
            },
        )?;
        let (d, v) = (Box::new(d1), Box::new(v));
        Ok(match kw {
            LetKw::Bool => ke!(ExLetBool {
                variable_expression: d,
                assignment_expression: v
            }),
            LetKw::Obj => ke!(ExLetObj {
                variable_expression: d,
                assignment_expression: v
            }),
            LetKw::Weak => ke!(ExLetWeakObjPtr {
                variable_expression: d,
                assignment_expression: v
            }),
            LetKw::Delegate => ke!(ExLetDelegate {
                variable_expression: d,
                assignment_expression: v
            }),
            _ => ke!(ExLetMulticastDelegate {
                variable_expression: d,
                assignment_expression: v
            }),
        })
    }

    pub fn stmt(&mut self, s: &Stmt, pos: u32) -> R<Option<KE>> {
        let b = Box::new;
        Ok(Some(match s {
            Stmt::Label(_) => return Ok(None),
            Stmt::Expr(e) => self.expr(e, Env::at(pos))?,
            Stmt::Let {
                kw,
                ptr,
                dest,
                value,
            } => self.let_stmt(*kw, ptr.as_ref(), dest, value, pos)?,
            Stmt::Goto(t) => ke!(ExJump {
                code_offset: self.target(t, false)?
            }),
            Stmt::GotoComputed(e) => ke!(ExComputedJump {
                code_offset_expression: b(self.expr(e, Env::at(pos + 1))?)
            }),
            Stmt::IfNot(c, t) => ke!(ExJumpIfNot {
                code_offset: self.target(t, false)?,
                boolean_expression: b(self.expr(c, Env::at(pos + 5))?)
            }),
            Stmt::Push(t) => ke!(ExPushExecutionFlow {
                pushing_address: self.target(t, false)?
            }),
            Stmt::Pop => ke!(ExPopExecutionFlow),
            Stmt::PopIfNot(c) => ke!(ExPopExecutionFlowIfNot {
                boolean_expression: b(self.expr(c, Env::at(pos + 1))?)
            }),
            Stmt::Return(e) => {
                let r = match e {
                    Some(e) => self.expr(e, Env::at(pos + 1))?,
                    None => ke!(ExNothing),
                };
                ke!(ExReturn {
                    return_expression: b(r)
                })
            }
            Stmt::Nop => ke!(ExNothing),
            Stmt::End => ke!(ExEndOfScript),
            Stmt::SetArray(d, v) => {
                let dv = self.expr(d, Env::at(pos + 1))?;
                let p = pos + 1 + self.size(&dv)?;
                let (elements, _) = self.seq(v, p)?;
                ke!(ExSetArray {
                    assigning_property: Some(b(dv)),
                    array_inner_prop: None,
                    elements: elements
                })
            }
            Stmt::SetSet(d, v) => {
                let dv = self.expr(d, Env::at(pos + 1))?;
                let p = pos + 5 + self.size(&dv)?;
                let (elements, _) = self.seq(v, p)?;
                ke!(ExSetSet {
                    set_property: b(dv),
                    elements: elements
                })
            }
            Stmt::SetMap(d, v) => {
                let dv = self.expr(d, Env::at(pos + 1))?;
                let p = pos + 5 + self.size(&dv)?;
                let flat: Vec<Expr> = v.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect();
                let (elements, _) = self.seq(&flat, p)?;
                ke!(ExSetMap {
                    map_property: b(dv),
                    elements: elements
                })
            }
        }))
    }
}
