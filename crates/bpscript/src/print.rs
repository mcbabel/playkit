use crate::ast::*;
use crate::lower::{dest_ptr, struct_parts, Env, Lower, KE, R};
use crate::names::{find_fname, fname_text, Want};
use crate::Asset;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use unreal_asset::kismet::walk::{Visitor, Walk};
use unreal_asset::kismet::*;
use unreal_asset::types::fname::FName;

pub struct Printer<'a> {
    pub low: Lower<'a>,
}

fn label(off: u32) -> String {
    format!("L{off}")
}

fn f32bits(v: f64) -> R<u32> {
    let f = v as f32;
    if f as f64 != v && !v.is_nan() {
        return Err(format!("constant {v} is not a 32 bit float"));
    }
    Ok(f.to_bits())
}

struct Targets<'a>(&'a mut BTreeSet<u32>);

impl Visitor for Targets<'_> {
    fn expr(&mut self, e: &mut KE) {
        match e {
            KE::ExJump(j) => {
                self.0.insert(j.code_offset);
            }
            KE::ExJumpIfNot(j) => {
                self.0.insert(j.code_offset);
            }
            KE::ExPushExecutionFlow(p) => {
                self.0.insert(p.pushing_address);
            }
            KE::ExSkipOffsetConst(s) => {
                self.0.insert(s.value);
            }
            _ => {}
        }
    }
}

struct Entries<'a> {
    uber: i32,
    out: &'a mut BTreeSet<u32>,
}

impl Visitor for Entries<'_> {
    fn expr(&mut self, e: &mut KE) {
        let (node, params) = match e {
            KE::ExLocalFinalFunction(f) => (f.stack_node.index, &f.parameters),
            KE::ExFinalFunction(f) => (f.stack_node.index, &f.parameters),
            KE::ExCallMath(f) => (f.stack_node.index, &f.parameters),
            _ => return,
        };
        if node == self.uber {
            for p in params {
                if let KE::ExIntConst(i) = p {
                    if i.value >= 0 {
                        self.out.insert(i.value as u32);
                    }
                }
            }
        }
    }
}

pub fn offsets(low: &Lower, code: &[KE]) -> R<Vec<u32>> {
    let mut out = Vec::with_capacity(code.len() + 1);
    let mut off = 0;
    for e in code {
        out.push(off);
        off += low.size(e)?;
    }
    out.push(off);
    Ok(out)
}

pub struct FuncCode {
    pub index: i32,
    pub code: Vec<KE>,
}

pub fn functions(a: &Asset) -> Vec<FuncCode> {
    a.asset_data
        .exports
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let code = struct_parts(e)?.script_bytecode.as_ref()?;
            if code.is_empty() {
                return None;
            }
            Some(FuncCode {
                index: i as i32 + 1,
                code: code.clone(),
            })
        })
        .collect()
}

impl<'a> Printer<'a> {
    pub fn new(a: &'a Asset) -> Self {
        Printer {
            low: Lower::new(a, false),
        }
    }

    fn pick(&mut self, orig: &KE, env: Env, cands: Vec<Expr>) -> R<Expr> {
        let mut last = String::new();
        for c in cands {
            match self.low.expr(&c, env) {
                Ok(e) if &e == orig => return Ok(c),
                Ok(_) => last = format!("{c} assembles differently"),
                Err(err) => last = err,
            }
        }
        Err(format!("cannot express {:?} ({last})", orig.get_token()))
    }

    fn pick_stmt(&mut self, orig: &KE, pos: u32, cands: Vec<Stmt>) -> R<Stmt> {
        let mut last = String::new();
        for c in cands {
            match self.low.stmt(&c, pos) {
                Ok(Some(e)) if &e == orig => return Ok(c),
                Ok(_) => last = format!("{c} assembles differently"),
                Err(err) => last = err,
            }
        }
        Err(format!("cannot express {:?} ({last})", orig.get_token()))
    }

    pub fn fname(&self, f: &FName) -> R<Name> {
        let a = self.low.asset;
        let n = Name::new(fname_text(f));
        if find_fname(a, &n).as_ref() == Some(f) {
            return Ok(n);
        }
        let n = Name {
            text: f.get_content(),
            number: Some(f.get_number()),
        };
        if find_fname(a, &n).as_ref() == Some(f) {
            return Ok(n);
        }
        Err(format!("name {} cannot be written", f.get_content()))
    }

    fn reference(&self, i: i32, want: Want) -> Ref {
        self.low.objs.print_ref(i, want)
    }

    fn full_ptr(&self, p: &KismetPropertyPointer) -> R<Ptr> {
        let f = p
            .new
            .as_ref()
            .ok_or_else(|| "old style property pointer".to_string())?;
        let owner = f.resolved_owner.index;
        if f.path.is_empty() && owner == 0 {
            return Ok(Ptr::null());
        }
        let path = f
            .path
            .iter()
            .map(|n| self.fname(n))
            .collect::<R<Vec<_>>>()?;
        Ok(Ptr {
            owner: Some(self.reference(owner, Want::Any)),
            path,
        })
    }

    fn bare_ptr(&self, p: &KismetPropertyPointer) -> Option<Ptr> {
        let f = p.new.as_ref()?;
        if f.path.len() != 1 {
            return None;
        }
        Some(Ptr::bare(self.fname(&f.path[0]).ok()?))
    }

    fn seq(&mut self, items: &[KE], mut pos: u32) -> R<Vec<Expr>> {
        let mut out = vec![];
        for x in items {
            out.push(self.expr(x, Env::at(pos))?);
            pos += self.low.size(x)?;
        }
        Ok(out)
    }

    fn target(&self, off: u32, uber: bool) -> Target {
        let l = label(off);
        let map = if uber {
            &self.low.uber_labels
        } else {
            &self.low.labels
        };
        if map.contains_key(&l) {
            Target::Label(l)
        } else {
            Target::Raw(off)
        }
    }

    fn var(&mut self, e: &KE, kind: VarKind, p: &KismetPropertyPointer, env: Env) -> R<Expr> {
        let full = self.full_ptr(p)?;
        let mut c = vec![];
        if let Some(b) = self.bare_ptr(p) {
            c.push(Expr::Var(b.clone()));
        }
        if full.path.len() == 1 {
            c.push(Expr::Var(full.clone()));
        }
        if let Some(b) = self.bare_ptr(p) {
            c.push(Expr::KindVar(kind, b));
        }
        c.push(Expr::KindVar(kind, full));
        self.pick(e, env, c)
    }

    fn call(&mut self, e: &KE, head: Ref, params: &[KE], env: Env, uber: bool) -> R<Expr> {
        let (kind, head_size) = match e {
            KE::ExCallMath(_) => (CallKind::Math, 9),
            KE::ExFinalFunction(_) => (CallKind::Final, 9),
            KE::ExLocalFinalFunction(_) => (CallKind::LocalFinal, 9),
            KE::ExVirtualFunction(_) => (CallKind::Virtual, 13),
            _ => (CallKind::LocalVirtual, 13),
        };
        let mut args = vec![];
        let mut pos = env.pos + head_size;
        for p in params {
            let a = match p {
                KE::ExIntConst(i) if uber && i.value >= 0 => {
                    let mut c = vec![];
                    let t = self.target(i.value as u32, true);
                    if matches!(t, Target::Label(_)) {
                        c.push(Expr::Addr(t));
                    }
                    c.push(Expr::Lit(Lit::Int(i.value)));
                    self.pick(
                        p,
                        Env {
                            uber_arg: true,
                            ..Env::at(pos)
                        },
                        c,
                    )?
                }
                _ => self.expr(
                    p,
                    Env {
                        uber_arg: uber,
                        ..Env::at(pos)
                    },
                )?,
            };
            pos += self.low.size(p)?;
            args.push(a);
        }
        let c = vec![
            Expr::Call {
                kind: None,
                head: head.clone(),
                args: args.clone(),
            },
            Expr::Call {
                kind: Some(kind),
                head,
                args,
            },
        ];
        self.pick(e, env, c)
    }

    pub fn expr(&mut self, e: &KE, env: Env) -> R<Expr> {
        let pos = env.pos;
        let b = Box::new;
        let lit = |l: Lit| Ok(Expr::Lit(l));
        match e {
            KE::ExLocalVariable(v) => self.var(e, VarKind::Local, &v.variable, env),
            KE::ExLocalOutVariable(v) => self.var(e, VarKind::Out, &v.variable, env),
            KE::ExInstanceVariable(v) => self.var(e, VarKind::Inst, &v.variable, env),
            KE::ExDefaultVariable(v) => self.var(e, VarKind::Default, &v.variable, env),
            KE::ExClassSparseDataVariable(v) => self.var(e, VarKind::Sparse, &v.variable, env),
            KE::ExObjectConst(o) => {
                let r = self.reference(o.value.index, Want::Any);
                let mut c = vec![];
                if let Ref::Path(s) = &r {
                    if s.len() == 1 {
                        c.push(Expr::Var(Ptr::bare(s[0].clone())));
                    }
                }
                c.push(Expr::Obj(r));
                self.pick(e, env, c)
            }
            KE::ExIntConst(i) => lit(Lit::Int(i.value)),
            KE::ExInt64Const(i) => lit(Lit::Int64(i.value)),
            KE::ExUInt64Const(i) => lit(Lit::UInt64(i.value)),
            KE::ExByteConst(i) => lit(Lit::Byte(i.value)),
            KE::ExIntConstByte(i) => lit(Lit::IntByte(i.value)),
            KE::ExIntZero(_) => lit(Lit::IntZero),
            KE::ExIntOne(_) => lit(Lit::IntOne),
            KE::ExFloatConst(f) => lit(Lit::Float(f.value.0.to_bits())),
            KE::ExTrue(_) => lit(Lit::True),
            KE::ExFalse(_) => lit(Lit::False),
            KE::ExSelf(_) => lit(Lit::SelfObj),
            KE::ExNoObject(_) => lit(Lit::NoObject),
            KE::ExNoInterface(_) => lit(Lit::NoInterface),
            KE::ExNothing(_) => lit(Lit::Nothing),
            KE::ExStringConst(s) => self.pick(
                e,
                env,
                vec![
                    Expr::Lit(Lit::Str(s.value.clone())),
                    Expr::Lit(Lit::AnsiStr(s.value.clone())),
                ],
            ),
            KE::ExUnicodeStringConst(s) => self.pick(
                e,
                env,
                vec![
                    Expr::Lit(Lit::Str(s.value.clone())),
                    Expr::Lit(Lit::UniStr(s.value.clone())),
                ],
            ),
            KE::ExNameConst(n) => lit(Lit::Name(self.fname(&n.value)?)),
            KE::ExVectorConst(v) => lit(Lit::Vector([
                f32bits(v.value.x.0)?,
                f32bits(v.value.y.0)?,
                f32bits(v.value.z.0)?,
            ])),
            KE::ExRotationConst(v) => lit(Lit::Rotator([
                f32bits(v.rotator.x.0)?,
                f32bits(v.rotator.y.0)?,
                f32bits(v.rotator.z.0)?,
            ])),
            KE::ExTransformConst(t) => {
                let v = &t.value;
                let mut a = [0u32; 10];
                for (k, x) in [
                    v.rotation.x,
                    v.rotation.y,
                    v.rotation.z,
                    v.rotation.w,
                    v.translation.x,
                    v.translation.y,
                    v.translation.z,
                    v.scale.x,
                    v.scale.y,
                    v.scale.z,
                ]
                .iter()
                .enumerate()
                {
                    a[k] = f32bits(x.0)?;
                }
                lit(Lit::Transform(a))
            }
            KE::ExEndParmValue(_) => lit(Lit::Token("EndParmValue".into())),
            KE::ExEndFunctionParms(_) => lit(Lit::Token("EndFunctionParms".into())),
            KE::ExEndStructConst(_) => lit(Lit::Token("EndStructConst".into())),
            KE::ExEndArray(_) => lit(Lit::Token("EndArray".into())),
            KE::ExEndArrayConst(_) => lit(Lit::Token("EndArrayConst".into())),
            KE::ExEndSet(_) => lit(Lit::Token("EndSet".into())),
            KE::ExEndSetConst(_) => lit(Lit::Token("EndSetConst".into())),
            KE::ExEndMap(_) => lit(Lit::Token("EndMap".into())),
            KE::ExEndMapConst(_) => lit(Lit::Token("EndMapConst".into())),
            KE::ExBreakpoint(_) => lit(Lit::Token("Breakpoint".into())),
            KE::ExTracepoint(_) => lit(Lit::Token("Tracepoint".into())),
            KE::ExWireTracepoint(_) => lit(Lit::Token("WireTracepoint".into())),
            KE::ExDeprecatedOp4A(_) => lit(Lit::Token("DeprecatedOp4A".into())),
            KE::ExFinalFunction(f) => {
                let uber = f.stack_node.index == self.low.uber && self.low.uber != 0;
                let head = self.reference(f.stack_node.index, Want::Function);
                self.call(e, head, &f.parameters, env, uber)
            }
            KE::ExLocalFinalFunction(f) => {
                let uber = f.stack_node.index == self.low.uber && self.low.uber != 0;
                let head = self.reference(f.stack_node.index, Want::Function);
                self.call(e, head, &f.parameters, env, uber)
            }
            KE::ExCallMath(f) => {
                let head = self.reference(f.stack_node.index, Want::Function);
                self.call(e, head, &f.parameters, env, false)
            }
            KE::ExVirtualFunction(f) => {
                let head = Ref::Path(vec![self.fname(&f.virtual_function_name)?]);
                self.call(e, head, &f.parameters, env, false)
            }
            KE::ExLocalVirtualFunction(f) => {
                let head = Ref::Path(vec![self.fname(&f.virtual_function_name)?]);
                self.call(e, head, &f.parameters, env, false)
            }
            KE::ExContext(c) => self.ctx(
                e,
                CtxOp::Normal,
                &c.object_expression,
                c.offset,
                &c.r_value_pointer,
                &c.context_expression,
                env,
            ),
            KE::ExContextFailSilent(c) => self.ctx(
                e,
                CtxOp::FailSilent,
                &c.object_expression,
                c.offset,
                &c.r_value_pointer,
                &c.context_expression,
                env,
            ),
            KE::ExClassContext(c) => self.ctx(
                e,
                CtxOp::Class,
                &c.object_expression,
                c.offset,
                &c.r_value_pointer,
                &c.context_expression,
                env,
            ),
            KE::ExStructMemberContext(m) => {
                let s = self.expr(&m.struct_expression, Env::at(pos + 9))?;
                let mut c = vec![];
                if let Some(bp) = self.bare_ptr(&m.struct_member_expression) {
                    c.push(Expr::Ctx {
                        obj: b(s.clone()),
                        op: CtxOp::Normal,
                        rv: None,
                        skip: None,
                        inner: b(Expr::Var(bp.clone())),
                    });
                    c.push(Expr::Member(b(s.clone()), bp));
                }
                c.push(Expr::Member(
                    b(s),
                    self.full_ptr(&m.struct_member_expression)?,
                ));
                self.pick(e, env, c)
            }
            KE::ExArrayGetByRef(x) => {
                let a = self.expr(&x.array_variable, Env::at(pos + 1))?;
                let p2 = pos + 1 + self.low.size(&x.array_variable)?;
                let i = self.expr(&x.array_index, Env::at(p2))?;
                self.pick(e, env, vec![Expr::Index(b(a), b(i))])
            }
            KE::ExDynamicCast(x) => self.cast(
                e,
                CastKind::Dynamic,
                x.class_ptr.index,
                &x.target_expression,
                env,
            ),
            KE::ExMetaCast(x) => self.cast(
                e,
                CastKind::Meta,
                x.class_ptr.index,
                &x.target_expression,
                env,
            ),
            KE::ExObjToInterfaceCast(x) => {
                self.cast(e, CastKind::ObjToIface, x.class_ptr.index, &x.target, env)
            }
            KE::ExCrossInterfaceCast(x) => {
                self.cast(e, CastKind::CrossIface, x.class_ptr.index, &x.target, env)
            }
            KE::ExInterfaceToObjCast(x) => {
                self.cast(e, CastKind::IfaceToObj, x.class_ptr.index, &x.target, env)
            }
            KE::ExPrimitiveCast(x) => {
                let t = self.expr(&x.target, Env::at(pos + 2))?;
                let name = format!("{:?}", x.conversion_type);
                self.pick(e, env, vec![Expr::PrimCast(name, b(t))])
            }
            KE::ExStructConst(s) => {
                let fields = self.seq(&s.value, pos + 13)?;
                let ty = self.reference(s.struct_value.index, Want::Struct);
                self.pick(
                    e,
                    env,
                    vec![
                        Expr::Struct {
                            ty: ty.clone(),
                            size: None,
                            fields: fields.clone(),
                        },
                        Expr::Struct {
                            ty,
                            size: Some(s.struct_size),
                            fields,
                        },
                    ],
                )
            }
            KE::ExSwitchValue(s) => {
                let mut p = pos + 7;
                let index = self.expr(&s.index_term, Env::at(p))?;
                p += self.low.size(&s.index_term)?;
                let mut cases = vec![];
                for c in &s.cases {
                    let ce = self.expr(&c.case_index_value_term, Env::at(p))?;
                    p += self.low.size(&c.case_index_value_term)? + 4;
                    let ve = self.expr(&c.case_term, Env::at(p))?;
                    p += self.low.size(&c.case_term)?;
                    cases.push((ce, ve));
                }
                let d = self.expr(&s.default_term, Env::at(p))?;
                self.pick(
                    e,
                    env,
                    vec![Expr::Switch {
                        index: b(index),
                        cases,
                        default: b(d),
                    }],
                )
            }
            KE::ExSkipOffsetConst(s) => {
                let t = self.target(s.value, false);
                let mut c = vec![];
                if matches!(t, Target::Label(_)) {
                    c.push(Expr::Addr(t.clone()));
                }
                c.push(Expr::SkipOffset(t));
                c.push(Expr::SkipOffset(Target::Raw(s.value)));
                self.pick(e, env, c)
            }
            KE::ExTextConst(t) => {
                let t = &t.value;
                let p = pos + 2;
                let need = |x: &Option<KE>| x.clone().ok_or_else(|| "incomplete text".to_string());
                let text = match t.text_literal_type {
                    EBlueprintTextLiteralType::Empty => Text::Empty,
                    EBlueprintTextLiteralType::LocalizedText => {
                        let v = self.seq(
                            &[
                                need(&t.localized_source)?,
                                need(&t.localized_key)?,
                                need(&t.localized_namespace)?,
                            ],
                            p,
                        )?;
                        Text::Localized(v[2].clone(), v[1].clone(), v[0].clone())
                    }
                    EBlueprintTextLiteralType::InvariantText => {
                        Text::Invariant(self.expr(&need(&t.invariant_literal_string)?, Env::at(p))?)
                    }
                    EBlueprintTextLiteralType::LiteralString => {
                        Text::Literal(self.expr(&need(&t.literal_string)?, Env::at(p))?)
                    }
                    EBlueprintTextLiteralType::StringTableEntry => {
                        let r = self.reference(
                            t.string_table_asset.map(|x| x.index).unwrap_or(0),
                            Want::Any,
                        );
                        let v = self.seq(
                            &[need(&t.string_table_id)?, need(&t.string_table_key)?],
                            p + 8,
                        )?;
                        Text::Table(r, v[0].clone(), v[1].clone())
                    }
                };
                self.pick(e, env, vec![Expr::Text(Box::new(text))])
            }
            KE::ExBindDelegate(x) => {
                let n = self.fname(&x.function_name)?;
                let d = self.expr(&x.delegate, Env::at(pos + 13))?;
                let p2 = pos + 13 + self.low.size(&x.delegate)?;
                let o = self.expr(&x.object_term, Env::at(p2))?;
                self.pick(e, env, vec![Expr::Bind(b(d), b(o), n)])
            }
            KE::ExAddMulticastDelegate(x) => {
                let d = self.expr(&x.delegate, Env::at(pos + 1))?;
                let p2 = pos + 1 + self.low.size(&x.delegate)?;
                let a = self.expr(&x.delegate_to_add, Env::at(p2))?;
                self.pick(e, env, vec![Expr::AddMulti(b(d), b(a))])
            }
            KE::ExRemoveMulticastDelegate(x) => {
                let d = self.expr(&x.delegate, Env::at(pos + 1))?;
                let p2 = pos + 1 + self.low.size(&x.delegate)?;
                let a = self.expr(&x.delegate_to_add, Env::at(p2))?;
                self.pick(e, env, vec![Expr::RemoveMulti(b(d), b(a))])
            }
            KE::ExClearMulticastDelegate(x) => {
                let d = self.expr(&x.delegate_to_clear, Env::at(pos + 1))?;
                self.pick(e, env, vec![Expr::ClearMulti(b(d))])
            }
            KE::ExCallMulticastDelegate(x) => {
                let d = self.expr(&x.delegate, Env::at(pos + 9))?;
                let p2 = pos + 9 + self.low.size(&x.delegate)?;
                let args = self.seq(&x.parameters, p2)?;
                let sig = self.reference(x.stack_node.index, Want::Function);
                self.pick(
                    e,
                    env,
                    vec![
                        Expr::Broadcast {
                            sig: None,
                            del: b(d.clone()),
                            args: args.clone(),
                        },
                        Expr::Broadcast {
                            sig: Some(sig),
                            del: b(d),
                            args,
                        },
                    ],
                )
            }
            KE::ExAssert(x) => {
                let a = self.expr(&x.assert_expression, Env::at(pos + 4))?;
                self.pick(
                    e,
                    env,
                    vec![Expr::Assert(b(a), x.line_number, x.debug_mode)],
                )
            }
            KE::ExInstrumentationEvent(x) => {
                let n = match &x.event_name {
                    Some(n) => Some(self.fname(n)?),
                    None => None,
                };
                self.pick(
                    e,
                    env,
                    vec![Expr::Instrument(format!("{:?}", x.event_type), n)],
                )
            }
            KE::ExFieldPathConst(x) => {
                let v = self.expr(&x.value, Env::at(pos + 1))?;
                self.pick(e, env, vec![Expr::FieldPath(b(v))])
            }
            KE::ExPropertyConst(x) => {
                let p = self.full_ptr(&x.property)?;
                self.pick(e, env, vec![Expr::Property(p)])
            }
            KE::ExSoftObjectConst(x) => {
                let v = self.expr(&x.value, Env::at(pos + 1))?;
                self.pick(e, env, vec![Expr::SoftObj(b(v))])
            }
            KE::ExInstanceDelegate(x) => {
                let n = self.fname(&x.function_name)?;
                self.pick(e, env, vec![Expr::Delegate(n)])
            }
            KE::ExInterfaceContext(x) => {
                let v = self.expr(&x.interface_value, Env::at(pos + 1))?;
                self.pick(e, env, vec![Expr::Interface(b(v))])
            }
            KE::ExSkip(x) => {
                let v = self.expr(&x.skip_expression, Env::at(pos + 5))?;
                self.pick(e, env, vec![Expr::Skip(b(v))])
            }
            KE::ExArrayConst(x) => {
                let p = self.full_ptr(&x.inner_property)?;
                let v = self.seq(&x.elements, pos + 13)?;
                self.pick(e, env, vec![Expr::ArrayConst(p, v)])
            }
            KE::ExSetConst(x) => {
                let p = self.full_ptr(&x.inner_property)?;
                let v = self.seq(&x.elements, pos + 13)?;
                self.pick(e, env, vec![Expr::SetConst(p, v)])
            }
            KE::ExMapConst(x) => {
                let k = self.full_ptr(&x.key_property)?;
                let vp = self.full_ptr(&x.value_property)?;
                let v = self.seq(&x.elements, pos + 21)?;
                if v.len() % 2 != 0 {
                    return Err("map constant with an odd number of elements".into());
                }
                let pairs = v.chunks(2).map(|c| (c[0].clone(), c[1].clone())).collect();
                self.pick(e, env, vec![Expr::MapConst(k, vp, pairs)])
            }
            _ => {
                let s = self.stmt(e, pos)?;
                self.pick(e, env, vec![Expr::Stmt(Box::new(s))])
            }
        }
    }

    fn cast(&mut self, e: &KE, kind: CastKind, class: i32, target: &KE, env: Env) -> R<Expr> {
        let t = self.expr(target, Env::at(env.pos + 9))?;
        let c = self.reference(class, Want::Class);
        self.pick(e, env, vec![Expr::Cast(kind, c, Box::new(t))])
    }

    #[allow(clippy::too_many_arguments)]
    fn ctx(
        &mut self,
        e: &KE,
        op: CtxOp,
        obj: &KE,
        skip: u32,
        rv: &KismetPropertyPointer,
        inner: &KE,
        env: Env,
    ) -> R<Expr> {
        let pos = env.pos;
        let o = self.expr(obj, Env::at(pos + 1))?;
        let ipos = pos + 1 + self.low.size(obj)? + 12;
        let i = self.expr(
            inner,
            Env {
                member: true,
                ..Env::at(ipos)
            },
        )?;
        let rvp = self.full_ptr(rv)?;
        let mk = |rv: Option<Ptr>, skip: Option<u32>| Expr::Ctx {
            obj: Box::new(o.clone()),
            op,
            rv,
            skip,
            inner: Box::new(i.clone()),
        };
        self.pick(
            e,
            env,
            vec![
                mk(None, None),
                mk(Some(rvp.clone()), None),
                mk(None, Some(skip)),
                mk(Some(rvp), Some(skip)),
            ],
        )
    }

    pub fn stmt(&mut self, e: &KE, pos: u32) -> R<Stmt> {
        match e {
            KE::ExLet(l) => {
                let d = self.expr(&l.variable, Env::at(pos + 9))?;
                let vpos = pos + 9 + self.low.size(&l.variable)?;
                let v = self.expr(
                    &l.expression,
                    Env {
                        rv: Some(&l.value),
                        ..Env::at(vpos)
                    },
                )?;
                let p = self.full_ptr(&l.value)?;
                let mk = |kw: Option<LetKw>, ptr: Option<Ptr>| Stmt::Let {
                    kw,
                    ptr,
                    dest: Box::new(d.clone()),
                    value: Box::new(v.clone()),
                };
                self.pick_stmt(
                    e,
                    pos,
                    vec![
                        mk(None, None),
                        mk(Some(LetKw::Let), None),
                        mk(Some(LetKw::Let), Some(p)),
                    ],
                )
            }
            KE::ExLetBool(l) => self.let_simple(
                e,
                LetKw::Bool,
                &l.variable_expression,
                &l.assignment_expression,
                pos,
            ),
            KE::ExLetObj(l) => self.let_simple(
                e,
                LetKw::Obj,
                &l.variable_expression,
                &l.assignment_expression,
                pos,
            ),
            KE::ExLetWeakObjPtr(l) => self.let_simple(
                e,
                LetKw::Weak,
                &l.variable_expression,
                &l.assignment_expression,
                pos,
            ),
            KE::ExLetDelegate(l) => self.let_simple(
                e,
                LetKw::Delegate,
                &l.variable_expression,
                &l.assignment_expression,
                pos,
            ),
            KE::ExLetMulticastDelegate(l) => self.let_simple(
                e,
                LetKw::Multicast,
                &l.variable_expression,
                &l.assignment_expression,
                pos,
            ),
            KE::ExLetValueOnPersistentFrame(l) => {
                let v = self.expr(
                    &l.assignment_expression,
                    Env {
                        rv: Some(&l.destination_property),
                        ..Env::at(pos + 9)
                    },
                )?;
                let full = self.full_ptr(&l.destination_property)?;
                let mut c = vec![];
                let mk = |kw: Option<LetKw>, p: Ptr| Stmt::Let {
                    kw,
                    ptr: None,
                    dest: Box::new(Expr::Var(p)),
                    value: Box::new(v.clone()),
                };
                if let Some(bp) = self.bare_ptr(&l.destination_property) {
                    c.push(mk(None, bp.clone()));
                    c.push(mk(Some(LetKw::Frame), bp));
                }
                if full.path.len() == 1 {
                    c.push(mk(None, full.clone()));
                }
                c.push(mk(Some(LetKw::Frame), full));
                self.pick_stmt(e, pos, c)
            }
            KE::ExJump(j) => Ok(Stmt::Goto(self.target(j.code_offset, false))),
            KE::ExJumpIfNot(j) => {
                let c = self.expr(&j.boolean_expression, Env::at(pos + 5))?;
                Ok(Stmt::IfNot(c, self.target(j.code_offset, false)))
            }
            KE::ExPushExecutionFlow(p) => Ok(Stmt::Push(self.target(p.pushing_address, false))),
            KE::ExPopExecutionFlow(_) => Ok(Stmt::Pop),
            KE::ExPopExecutionFlowIfNot(p) => Ok(Stmt::PopIfNot(
                self.expr(&p.boolean_expression, Env::at(pos + 1))?,
            )),
            KE::ExReturn(r) => match r.return_expression.as_ref() {
                KE::ExNothing(_) => Ok(Stmt::Return(None)),
                x => Ok(Stmt::Return(Some(self.expr(x, Env::at(pos + 1))?))),
            },
            KE::ExComputedJump(j) => Ok(Stmt::GotoComputed(
                self.expr(&j.code_offset_expression, Env::at(pos + 1))?,
            )),
            KE::ExNothing(_) => Ok(Stmt::Nop),
            KE::ExEndOfScript(_) => Ok(Stmt::End),
            KE::ExSetArray(s) => {
                let d = s
                    .assigning_property
                    .as_ref()
                    .ok_or_else(|| "old style array assignment".to_string())?;
                let dv = self.expr(d, Env::at(pos + 1))?;
                let v = self.seq(&s.elements, pos + 1 + self.low.size(d)?)?;
                self.pick_stmt(e, pos, vec![Stmt::SetArray(dv, v)])
            }
            KE::ExSetSet(s) => {
                let dv = self.expr(&s.set_property, Env::at(pos + 1))?;
                let v = self.seq(&s.elements, pos + 5 + self.low.size(&s.set_property)?)?;
                self.pick_stmt(e, pos, vec![Stmt::SetSet(dv, v)])
            }
            KE::ExSetMap(s) => {
                let dv = self.expr(&s.map_property, Env::at(pos + 1))?;
                let v = self.seq(&s.elements, pos + 5 + self.low.size(&s.map_property)?)?;
                if v.len() % 2 != 0 {
                    return Err("map with an odd number of elements".into());
                }
                let pairs = v.chunks(2).map(|c| (c[0].clone(), c[1].clone())).collect();
                self.pick_stmt(e, pos, vec![Stmt::SetMap(dv, pairs)])
            }
            _ => Ok(Stmt::Expr(self.expr(e, Env::at(pos))?)),
        }
    }

    fn let_simple(&mut self, e: &KE, kw: LetKw, dest: &KE, value: &KE, pos: u32) -> R<Stmt> {
        let d = self.expr(dest, Env::at(pos + 1))?;
        let vpos = pos + 1 + self.low.size(dest)?;
        let rv = dest_ptr(dest).cloned();
        let v = self.expr(
            value,
            Env {
                rv: rv.as_ref(),
                ..Env::at(vpos)
            },
        )?;
        let mk = |k: Option<LetKw>| Stmt::Let {
            kw: k,
            ptr: None,
            dest: Box::new(d.clone()),
            value: Box::new(v.clone()),
        };
        self.pick_stmt(e, pos, vec![mk(None), mk(Some(kw))])
    }

    pub fn function_name(&self, idx: i32) -> Ref {
        self.reference(idx, Want::Export)
    }

    pub fn function(
        &mut self,
        f: &FuncCode,
        targets: &BTreeSet<u32>,
        uber_offsets: &HashMap<String, u32>,
    ) -> R<Function> {
        self.low.set_function(f.index);
        let offs = offsets(&self.low, &f.code)?;
        self.low.labels = offs.iter().map(|&o| (label(o), o)).collect();
        self.low.uber_labels = uber_offsets.clone();
        let n = f.code.len();
        let noend = !matches!(f.code.last(), Some(KE::ExEndOfScript(_)));
        let body_len = if noend { n } else { n - 1 };
        let mut body = vec![];
        for (e, &off) in f.code[..body_len].iter().zip(&offs) {
            if targets.contains(&off) {
                body.push(Stmt::Label(label(off)));
            }
            body.push(self.stmt(e, off).map_err(|e| format!("at {off}: {e}"))?);
        }
        if targets.contains(&offs[body_len]) {
            body.push(Stmt::Label(label(offs[body_len])));
        }
        Ok(Function {
            name: self.function_name(f.index),
            noend,
            body,
        })
    }
}

pub fn disassemble(a: &Asset) -> R<String> {
    let funcs = functions(a);
    let mut p = Printer::new(a);
    let mut targets: BTreeMap<i32, BTreeSet<u32>> = BTreeMap::new();
    let mut offs_of: HashMap<i32, Vec<u32>> = HashMap::new();
    for f in &funcs {
        let mut t = BTreeSet::new();
        for x in &f.code {
            x.clone().walk(&mut Targets(&mut t));
        }
        targets.insert(f.index, t);
        offs_of.insert(f.index, offsets(&p.low, &f.code)?);
    }
    for f in &funcs {
        p.low.set_function(f.index);
        let uber = p.low.uber;
        if uber == 0 || uber == f.index || !offs_of.contains_key(&uber) {
            continue;
        }
        let mut entries = BTreeSet::new();
        for x in &f.code {
            x.clone().walk(&mut Entries {
                uber,
                out: &mut entries,
            });
        }
        let offs = &offs_of[&uber];
        let set = targets.entry(uber).or_default();
        for v in entries {
            if offs.contains(&v) {
                set.insert(v);
            }
        }
    }
    let mut out = String::new();
    for f in &funcs {
        p.low.set_function(f.index);
        let uber = p.low.uber;
        let uber_offsets: HashMap<String, u32> = match offs_of.get(&uber) {
            Some(o) if uber != 0 => o.iter().map(|&x| (label(x), x)).collect(),
            _ => HashMap::new(),
        };
        let func = p
            .function(f, &targets[&f.index], &uber_offsets)
            .map_err(|e| format!("{}: {e}", p.low.objs.name(f.index)))?;
        out.push_str(&func.to_string());
        out.push('\n');
    }
    Ok(out)
}
