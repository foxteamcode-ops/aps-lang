use crate::compiler::ast::*;
use crate::compiler::types::{Type, TypeContext};

#[derive(Debug, Clone)]
pub struct HirModule {
    pub structs: Vec<HirStruct>,
    pub enums: Vec<HirEnum>,
    pub consts: Vec<(String, Type, Expr)>,
    pub externs: Vec<FnSig>,
    pub functions: Vec<HirFunction>,
}

#[derive(Debug, Clone)]
pub struct HirStruct {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone)]
pub struct HirEnum {
    pub name: String,
    pub variants: Vec<(String, Vec<Type>)>,
}

#[derive(Debug, Clone)]
pub struct HirFunction {
    pub name: String,
    pub params: Vec<(String, Capability, Type, bool)>,
    pub ret_type: Type,
    pub is_async: bool,
    pub body: Block,
}

pub struct HirLowering {
    pub tcx: TypeContext,
}

impl HirLowering {
    pub fn new(tcx: TypeContext) -> Self {
        Self { tcx }
    }

    pub fn lower_program(&self, program: &Program) -> HirModule {
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut consts = Vec::new();
        let mut externs = Vec::new();
        let mut functions = Vec::new();

        for item in &program.items {
            match item {
                Item::Struct(s) => {
                    let fields = s
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), self.tcx.resolve_type_ast(&f.ty).1))
                        .collect();
                    structs.push(HirStruct {
                        name: s.name.clone(),
                        fields,
                    });
                }
                Item::Enum(e) => {
                    let variants = e
                        .variants
                        .iter()
                        .map(|v| {
                            let payload = match &v.kind {
                                EnumVariantKind::Unit => Vec::new(),
                                EnumVariantKind::Tuple(ts) => {
                                    ts.iter().map(|t| self.tcx.resolve_type_ast(t).1).collect()
                                }
                                EnumVariantKind::Struct(fs) => fs
                                    .iter()
                                    .map(|f| self.tcx.resolve_type_ast(&f.ty).1)
                                    .collect(),
                            };
                            (v.name.clone(), payload)
                        })
                        .collect();
                    enums.push(HirEnum {
                        name: e.name.clone(),
                        variants,
                    });
                }
                Item::Const(c) => {
                    let (_, mut ty) = self.tcx.resolve_type_ast(&c.ty);
                    if ty == Type::Unknown {
                        ty = match &c.value.kind {
                            ExprKind::FloatLit(_, _) => Type::F64,
                            ExprKind::StringLit(_) => Type::String,
                            ExprKind::BoolLit(_) => Type::Bool,
                            _ => Type::I64,
                        };
                    }
                    consts.push((c.name.clone(), ty, c.value.clone()));
                }
                Item::Extern(ext) => {
                    for sig in &ext.functions {
                        externs.push(sig.clone());
                    }
                }
                Item::Function(f) => {
                    functions.push(self.lower_fn(f, None));
                }
                Item::Impl(imp) => {
                    for m in &imp.methods {
                        functions.push(self.lower_fn(m, Some(&imp.target_name)));
                    }
                }
                _ => {}
            }
        }

        HirModule {
            structs,
            enums,
            consts,
            externs,
            functions,
        }
    }

    fn lower_fn(&self, f: &FnDecl, impl_target: Option<&str>) -> HirFunction {
        let name = if let Some(tgt) = impl_target {
            format!("{}__{}", tgt, f.name)
        } else {
            f.name.clone()
        };

        let mut params = Vec::new();
        for p in &f.params {
            let (cap_t, mut ty) = self.tcx.resolve_type_ast(&p.ty);
            if let Some(tgt) = impl_target {
                if p.name == "self"
                    && matches!(ty, Type::Struct(ref n, _) if n == "Self" || n == tgt)
                {
                    ty = Type::Struct(tgt.to_string(), Vec::new());
                }
            }
            let cap = if p.capability != Capability::Value {
                p.capability
            } else {
                cap_t
            };
            params.push((p.name.clone(), cap, ty, p.is_mutable || cap == Capability::Write));
        }

        let (_, mut ret_type) = self.tcx.resolve_type_ast(&f.ret_type);
        if let Some(tgt) = impl_target {
            if matches!(ret_type, Type::Struct(ref n, _) if n == "Self") {
                ret_type = Type::Struct(tgt.to_string(), Vec::new());
            }
        }

        HirFunction {
            name,
            params,
            ret_type,
            is_async: f.is_async,
            body: self.desugar_block(&f.body),
        }
    }

    fn desugar_block(&self, block: &Block) -> Block {
        let mut stmts = Vec::new();
        for stmt in &block.stmts {
            match stmt {
                // Desugar `for i in start..end { body }` into a `var i = start; while i < end { body; i += 1 }`
                Stmt::For {
                    var,
                    iter,
                    body,
                    span,
                } => {
                    if let ExprKind::Range {
                        start,
                        end,
                        inclusive,
                    } = &iter.kind
                    {
                        stmts.push(Stmt::Var {
                            name: var.clone(),
                            ty: TypeAst::Named {
                                name: "i64".to_string(),
                                args: Vec::new(),
                            },
                            init: (**start).clone(),
                            span: span.clone(),
                        });
                        let cmp_op = if *inclusive { BinOp::Le } else { BinOp::Lt };
                        let cond = Expr {
                            kind: ExprKind::Binary {
                                op: cmp_op,
                                left: Box::new(Expr {
                                    kind: ExprKind::Ident(var.clone()),
                                    span: span.clone(),
                                }),
                                right: end.clone(),
                            },
                            span: span.clone(),
                        };
                        let mut loop_body = self.desugar_block(body);
                        loop_body.stmts.push(Stmt::Assign {
                            target: Expr {
                                kind: ExprKind::Ident(var.clone()),
                                span: span.clone(),
                            },
                            op: AssignOp::AddAssign,
                            value: Expr {
                                kind: ExprKind::IntLit(1, None),
                                span: span.clone(),
                            },
                            span: span.clone(),
                        });
                        stmts.push(Stmt::While {
                            cond,
                            body: loop_body,
                            span: span.clone(),
                        });
                    } else {
                        stmts.push(Stmt::For {
                            var: var.clone(),
                            iter: iter.clone(),
                            body: self.desugar_block(body),
                            span: span.clone(),
                        });
                    }
                }
                Stmt::While { cond, body, span } => {
                    stmts.push(Stmt::While {
                        cond: cond.clone(),
                        body: self.desugar_block(body),
                        span: span.clone(),
                    });
                }
                Stmt::Loop { body, span } => {
                    stmts.push(Stmt::Loop {
                        body: self.desugar_block(body),
                        span: span.clone(),
                    });
                }
                Stmt::Region { name, body, span } => {
                    stmts.push(Stmt::Region {
                        name: name.clone(),
                        body: self.desugar_block(body),
                        span: span.clone(),
                    });
                }
                Stmt::Defer { body, span } => {
                    stmts.push(Stmt::Defer {
                        body: self.desugar_block(body),
                        span: span.clone(),
                    });
                }
                Stmt::Unsafe { body, span } => {
                    stmts.push(Stmt::Unsafe {
                        body: self.desugar_block(body),
                        span: span.clone(),
                    });
                }
                other => stmts.push(other.clone()),
            }
        }
        Block {
            stmts,
            span: block.span.clone(),
        }
    }
}
