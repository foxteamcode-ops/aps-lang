use crate::compiler::ast::*;
use crate::compiler::diagnostics::{Diagnostic, DiagnosticBag, Span};
use crate::compiler::types::{Type, TypeContext};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct VarSymbol {
    pub name: String,
    pub ty: Type,
    pub is_mutable: bool,
    pub capability: Capability,
    pub region_origin: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnSymbol {
    pub name: String,
    pub generics: Vec<String>,
    pub params: Vec<(String, Capability, Type)>,
    pub ret_type: Type,
    pub is_async: bool,
    pub is_extern: bool,
}

pub struct SemanticAnalyzer {
    pub tcx: TypeContext,
    pub functions: HashMap<String, FnSymbol>,
    pub methods: HashMap<(String, String), FnSymbol>,
    pub scopes: Vec<HashMap<String, VarSymbol>>,
    pub current_fn_ret: Type,
    pub in_async_fn: bool,
    pub in_unsafe_block: bool,
    pub in_loop_depth: usize,
    pub active_regions: Vec<String>,
    pub traits: HashMap<String, TraitDecl>,
    pub diagnostics: DiagnosticBag,
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        let mut sa = Self {
            tcx: TypeContext::new(),
            functions: HashMap::new(),
            methods: HashMap::new(),
            scopes: vec![HashMap::new()],
            current_fn_ret: Type::Void,
            in_async_fn: false,
            in_unsafe_block: false,
            in_loop_depth: 0,
            active_regions: Vec::new(),
            traits: HashMap::new(),
            diagnostics: DiagnosticBag::new(),
        };
        sa.register_builtins();
        sa
    }

    fn register_builtins(&mut self) {
        // Built-in functions
        let builtins = vec![
            ("print", vec![("val", Capability::Read, Type::Unknown)], Type::Void),
            ("println", vec![("val", Capability::Read, Type::Unknown)], Type::Void),
            ("eprintln", vec![("val", Capability::Read, Type::Unknown)], Type::Void),
            ("panic", vec![("msg", Capability::Read, Type::String)], Type::Never),
            ("assert", vec![("cond", Capability::Value, Type::Bool)], Type::Void),
            (
                "assert_eq",
                vec![
                    ("left", Capability::Read, Type::Unknown),
                    ("right", Capability::Read, Type::Unknown),
                ],
                Type::Void,
            ),
            (
                "share",
                vec![("val", Capability::Take, Type::Unknown)],
                Type::Shared(Box::new(Type::Unknown)),
            ),
            ("format", vec![("val", Capability::Read, Type::Unknown)], Type::String),
            ("to_string", vec![("val", Capability::Read, Type::Unknown)], Type::String),
            ("abs", vec![("x", Capability::Value, Type::I64)], Type::I64),
            ("min", vec![("a", Capability::Value, Type::I64), ("b", Capability::Value, Type::I64)], Type::I64),
            ("max", vec![("a", Capability::Value, Type::I64), ("b", Capability::Value, Type::I64)], Type::I64),
            ("sqrt", vec![("x", Capability::Value, Type::F64)], Type::F64),
            ("sleep_ms", vec![("ms", Capability::Value, Type::I64)], Type::Void),
            ("time_now_ms", vec![], Type::I64),
            ("time_now_ns", vec![], Type::I64),
            ("aps_time_now_ms", vec![], Type::I64),
            ("aps_time_now_ns", vec![], Type::I64),
            ("sha256_hex", vec![("data", Capability::Read, Type::String)], Type::String),
            ("random_i64", vec![("min", Capability::Value, Type::I64), ("max", Capability::Value, Type::I64)], Type::I64),
            ("fs_read_text", vec![("path", Capability::Read, Type::String)], Type::Result(Box::new(Type::String), Box::new(Type::String))),
            ("fs_write_text", vec![("path", Capability::Read, Type::String), ("content", Capability::Read, Type::String)], Type::Result(Box::new(Type::Void), Box::new(Type::String))),
            ("fs_exists", vec![("path", Capability::Read, Type::String)], Type::Bool),
            ("async_sleep_ms", vec![("ms", Capability::Value, Type::I64)], Type::I64),
            ("block_on", vec![("future", Capability::Value, Type::Unknown)], Type::Unknown),
            ("channel", vec![("cap", Capability::Value, Type::I64)], Type::Channel(Box::new(Type::Unknown))),
            ("spawn_thread", vec![("task", Capability::Value, Type::Unknown)], Type::Task(Box::new(Type::Unknown))),
            ("clock_nanos", vec![], Type::I64),
            ("simd_dot_f64", vec![("a", Capability::Read, Type::Unknown), ("b", Capability::Read, Type::Unknown), ("len", Capability::Value, Type::I64)], Type::F64),
            ("simd_sum_i64", vec![("a", Capability::Read, Type::Unknown), ("len", Capability::Value, Type::I64)], Type::I64),
            ("simd_min_i64", vec![("a", Capability::Read, Type::Unknown), ("len", Capability::Value, Type::I64)], Type::I64),
            ("simd_max_i64", vec![("a", Capability::Read, Type::Unknown), ("len", Capability::Value, Type::I64)], Type::I64),
            ("simd_find_byte", vec![("data", Capability::Read, Type::Unknown), ("len", Capability::Value, Type::I64), ("byte", Capability::Value, Type::I64)], Type::I64),
            ("simd_count_byte", vec![("data", Capability::Read, Type::Unknown), ("len", Capability::Value, Type::I64), ("byte", Capability::Value, Type::I64)], Type::I64),
            ("simd_is_ascii", vec![("data", Capability::Read, Type::Unknown), ("len", Capability::Value, Type::I64)], Type::Bool),
            ("str_slice", vec![("s", Capability::Read, Type::String), ("start", Capability::Value, Type::I64), ("len", Capability::Value, Type::I64)], Type::String),
            ("str_find", vec![("haystack", Capability::Read, Type::String), ("needle", Capability::Read, Type::String)], Type::I64),
            ("str_find_byte", vec![("s", Capability::Read, Type::String), ("byte", Capability::Value, Type::I64)], Type::I64),
            ("str_view_eq", vec![("a", Capability::Read, Type::String), ("a_len", Capability::Value, Type::I64), ("b", Capability::Read, Type::String), ("b_len", Capability::Value, Type::I64)], Type::Bool),
            ("likely", vec![("cond", Capability::Value, Type::Bool)], Type::Bool),
            ("unlikely", vec![("cond", Capability::Value, Type::Bool)], Type::Bool),
            ("http_response_fast", vec![("status", Capability::Value, Type::I64), ("content_type", Capability::Read, Type::String), ("body", Capability::Read, Type::String)], Type::String),
            ("parse_i64", vec![("s", Capability::Read, Type::String)], Type::I64),
            ("str_to_i64", vec![("s", Capability::Read, Type::String)], Type::I64),
            ("http_server_serve", vec![("port", Capability::Value, Type::I64), ("dispatch_fn", Capability::Value, Type::Unknown), ("max_requests", Capability::Value, Type::I64)], Type::I64),
            ("http_client_request", vec![("method", Capability::Read, Type::String), ("host", Capability::Read, Type::String), ("port", Capability::Value, Type::I64), ("path", Capability::Read, Type::String), ("body", Capability::Read, Type::String)], Type::Struct("Response".to_string(), Vec::new())),
            ("promote", vec![("val", Capability::Read, Type::Unknown)], Type::Unknown),
            ("region_set_quota", vec![("region", Capability::Read, Type::Unknown), ("max_bytes", Capability::Value, Type::I64)], Type::Void),
        ];

        for (name, params, ret) in builtins {
            self.functions.insert(
                name.to_string(),
                FnSymbol {
                    name: name.to_string(),
                    generics: Vec::new(),
                    params: params
                        .into_iter()
                        .map(|(n, c, t)| (n.to_string(), c, t))
                        .collect(),
                    ret_type: ret,
                    is_async: false,
                    is_extern: false,
                },
            );
        }

        // Built-in enums Option and Result constructors
        self.tcx.known_enums.insert(
            "Option".to_string(),
            vec![
                ("Some".to_string(), vec![Type::GenericParam("T".to_string())]),
                ("None".to_string(), vec![]),
            ],
        );
        self.tcx.known_enums.insert(
            "Result".to_string(),
            vec![
                ("Ok".to_string(), vec![Type::GenericParam("T".to_string())]),
                ("Err".to_string(), vec![Type::GenericParam("E".to_string())]),
            ],
        );
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_var(&mut self, sym: VarSymbol) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(sym.name.clone(), sym);
        }
    }

    pub fn lookup_var(&self, name: &str) -> Option<&VarSymbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.get(name) {
                return Some(sym);
            }
        }
        None
    }

    pub fn analyze_program(&mut self, program: &Program) {
        // Pass 1: Register structs, enums, type aliases, constants, functions, impls, externs
        for item in &program.items {
            match item {
                Item::Struct(s) => {
                    self.tcx.active_generics = s.generics.iter().map(|g| g.name.clone()).collect();
                    let mut fields = Vec::new();
                    for f in &s.fields {
                        let (_, fty) = self.tcx.resolve_type_ast(&f.ty);
                        fields.push((f.name.clone(), fty));
                    }
                    self.tcx.known_structs.insert(s.name.clone(), fields);
                    self.tcx.active_generics.clear();
                }
                Item::Enum(e) => {
                    self.tcx.active_generics = e.generics.iter().map(|g| g.name.clone()).collect();
                    let mut vars = Vec::new();
                    for v in &e.variants {
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
                        vars.push((v.name.clone(), payload));
                    }
                    self.tcx.known_enums.insert(e.name.clone(), vars);
                    self.tcx.active_generics.clear();
                }
                Item::TypeAlias { name, ty, .. } => {
                    let (_, resolved) = self.tcx.resolve_type_ast(ty);
                    self.tcx.type_aliases.insert(name.clone(), resolved);
                }
                Item::Trait(tr) => {
                    self.traits.insert(tr.name.clone(), tr.clone());
                }
                _ => {}
            }
        }

        for item in &program.items {
            match item {
                Item::Function(f) => {
                    self.register_fn_decl(f, false);
                }
                Item::Impl(imp) => {
                    self.tcx.active_generics =
                        imp.generics.iter().map(|g| g.name.clone()).collect();
                    if let Some(ref tr_name) = imp.trait_name {
                        if let Some(trait_decl) = self.traits.get(tr_name).cloned() {
                            for req_m in &trait_decl.methods {
                                let found = imp.methods.iter().find(|m| m.name == req_m.name);
                                if let Some(m) = found {
                                    if m.params.len() != req_m.params.len() {
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                "E0110",
                                                format!(
                                                    "method `{}` in `impl {} for {}` has {} parameter(s), expected {}",
                                                    m.name, tr_name, imp.target_name, m.params.len(), req_m.params.len()
                                                ),
                                                m.span.clone(),
                                            ),
                                        );
                                    }
                                } else {
                                    self.diagnostics.push(
                                        Diagnostic::error(
                                            "E0110",
                                            format!(
                                                "type `{}` does not implement required method `{}` from trait `{}`",
                                                imp.target_name, req_m.name, tr_name
                                            ),
                                            imp.span.clone(),
                                        )
                                        .with_help(format!(
                                            "implement `fn {}(...)` inside `impl {} for {}`",
                                            req_m.name, tr_name, imp.target_name
                                        )),
                                    );
                                }
                            }
                        } else {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0102",
                                    format!("undefined trait `{}`", tr_name),
                                    imp.span.clone(),
                                ),
                            );
                        }
                    }
                    for m in &imp.methods {
                        let sym = self.build_fn_symbol(m, Some(&imp.target_name), false);
                        self.methods
                            .insert((imp.target_name.clone(), m.name.clone()), sym.clone());
                        self.functions
                            .insert(format!("{}::{}", imp.target_name, m.name), sym);
                    }
                    self.tcx.active_generics.clear();
                }
                Item::Extern(ext) => {
                    for sig in &ext.functions {
                        self.tcx.active_generics =
                            sig.generics.iter().map(|g| g.name.clone()).collect();
                        let mut params = Vec::new();
                        for p in &sig.params {
                            let (cap_t, mut ty) = self.tcx.resolve_type_ast(&p.ty);
                            let cap = if p.capability != Capability::Value {
                                p.capability
                            } else {
                                cap_t
                            };
                            if matches!(ty, Type::Struct(ref n, _) if n == "Self") {
                                ty = Type::Unknown;
                            }
                            params.push((p.name.clone(), cap, ty));
                        }
                        let (_, ret_type) = self.tcx.resolve_type_ast(&sig.ret_type);
                        self.functions.insert(
                            sig.name.clone(),
                            FnSymbol {
                                name: sig.name.clone(),
                                generics: sig.generics.iter().map(|g| g.name.clone()).collect(),
                                params,
                                ret_type,
                                is_async: false,
                                is_extern: true,
                            },
                        );
                        self.tcx.active_generics.clear();
                    }
                }
                Item::Const(c) => {
                    let (_, mut ty) = self.tcx.resolve_type_ast(&c.ty);
                    let val_ty = self.infer_expr(&c.value);
                    if ty == Type::Unknown {
                        ty = val_ty;
                    }
                    self.define_var(VarSymbol {
                        name: c.name.clone(),
                        ty,
                        is_mutable: false,
                        capability: Capability::Value,
                        region_origin: None,
                        span: c.span.clone(),
                    });
                }
                _ => {}
            }
        }

        // Pass 2: Check bodies
        for item in &program.items {
            match item {
                Item::Function(f) => self.check_fn_decl(f, None),
                Item::Impl(imp) => {
                    for m in &imp.methods {
                        self.check_fn_decl(m, Some(&imp.target_name));
                    }
                }
                _ => {}
            }
        }
    }

    fn build_fn_symbol(
        &mut self,
        f: &FnDecl,
        self_type: Option<&str>,
        is_extern: bool,
    ) -> FnSymbol {
        self.tcx.active_generics = f.generics.iter().map(|g| g.name.clone()).collect();
        let mut params = Vec::new();
        for p in &f.params {
            let (cap_t, mut ty) = self.tcx.resolve_type_ast(&p.ty);
            if let Some(st) = self_type {
                if p.name == "self"
                    && matches!(ty, Type::Struct(ref n, _) if n == "Self" || n == st)
                {
                    ty = Type::Struct(st.to_string(), Vec::new());
                }
            }
            let mut cap = if p.capability != Capability::Value {
                p.capability
            } else {
                cap_t
            };
            if cap == Capability::Value && !ty.is_copy() && p.name == "self" {
                cap = Capability::Read;
            }
            params.push((p.name.clone(), cap, ty));
        }
        let (_, mut ret_type) = self.tcx.resolve_type_ast(&f.ret_type);
        if let Some(st) = self_type {
            if matches!(ret_type, Type::Struct(ref n, _) if n == "Self") {
                ret_type = Type::Struct(st.to_string(), Vec::new());
            }
        }
        self.tcx.active_generics.clear();
        FnSymbol {
            name: f.name.clone(),
            generics: f.generics.iter().map(|g| g.name.clone()).collect(),
            params,
            ret_type,
            is_async: f.is_async,
            is_extern,
        }
    }

    fn register_fn_decl(&mut self, f: &FnDecl, is_extern: bool) {
        let sym = self.build_fn_symbol(f, None, is_extern);
        self.functions.insert(f.name.clone(), sym);
    }

    fn check_fn_decl(&mut self, f: &FnDecl, self_type: Option<&str>) {
        let sym = self.build_fn_symbol(f, self_type, false);
        self.tcx.active_generics = sym.generics.clone();
        self.current_fn_ret = sym.ret_type.clone();
        let prev_async = self.in_async_fn;
        self.in_async_fn = f.is_async;

        self.push_scope();
        for (i, (pname, cap, pty)) in sym.params.iter().enumerate() {
            let is_mut = f.params.get(i).map(|p| p.is_mutable).unwrap_or(false)
                || *cap == Capability::Write;
            let pspan = f
                .params
                .get(i)
                .map(|p| p.span.clone())
                .unwrap_or_else(|| f.span.clone());
            self.define_var(VarSymbol {
                name: pname.clone(),
                ty: pty.clone(),
                is_mutable: is_mut,
                capability: *cap,
                region_origin: None,
                span: pspan,
            });
        }

        let body_ty = self.check_block(&f.body);
        if self.current_fn_ret != Type::Void
            && body_ty != Type::Void
            && !self.current_fn_ret.is_compatible_with(&body_ty)
        {
            self.diagnostics.push(
                Diagnostic::error(
                    "E0101",
                    format!(
                        "mismatched return type in function `{}`: expected `{}`, found `{}`",
                        f.name, self.current_fn_ret, body_ty
                    ),
                    f.span.clone(),
                )
                .with_help(format!(
                    "change the return expression to match `{}`",
                    self.current_fn_ret
                )),
            );
        }

        self.pop_scope();
        self.in_async_fn = prev_async;
        self.tcx.active_generics.clear();
    }

    pub fn check_block(&mut self, block: &Block) -> Type {
        self.push_scope();
        let mut last_ty = Type::Void;
        for (idx, stmt) in block.stmts.iter().enumerate() {
            let is_last = idx + 1 == block.stmts.len();
            let t = self.check_stmt(stmt);
            if is_last {
                if let Stmt::Expr { has_semi: false, .. } = stmt {
                    last_ty = t;
                }
            }
        }
        self.pop_scope();
        last_ty
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> Type {
        match stmt {
            Stmt::Let {
                name,
                ty,
                init,
                span,
            } => {
                let (_, declared_ty) = self.tcx.resolve_type_ast(ty);
                let init_ty = self.infer_expr(init);
                let final_ty = if declared_ty != Type::Unknown {
                    if !declared_ty.is_compatible_with(&init_ty) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0101",
                                format!(
                                    "type mismatch for variable `{}`: expected `{}`, found `{}`",
                                    name, declared_ty, init_ty
                                ),
                                span.clone(),
                            )
                            .with_label(format!("expected `{}`", declared_ty))
                            .with_help(
                                "use an explicit type conversion with `as` or fix the initializer",
                            ),
                        );
                    }
                    // Preserve RegionBound wrapper if init was allocated inside a region
                    if let Type::RegionBound { region, .. } = init_ty {
                        Type::RegionBound {
                            region,
                            inner: Box::new(declared_ty),
                        }
                    } else {
                        declared_ty
                    }
                } else {
                    init_ty
                };

                let region_origin = if let Type::RegionBound { ref region, .. } = final_ty {
                    Some(region.clone())
                } else {
                    None
                };

                self.define_var(VarSymbol {
                    name: name.clone(),
                    ty: final_ty,
                    is_mutable: false,
                    capability: Capability::Value,
                    region_origin,
                    span: span.clone(),
                });
                Type::Void
            }
            Stmt::Var {
                name,
                ty,
                init,
                span,
            } => {
                let (_, declared_ty) = self.tcx.resolve_type_ast(ty);
                let init_ty = self.infer_expr(init);
                let final_ty = if declared_ty != Type::Unknown {
                    if !declared_ty.is_compatible_with(&init_ty) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0101",
                                format!(
                                    "type mismatch for mutable variable `{}`: expected `{}`, found `{}`",
                                    name, declared_ty, init_ty
                                ),
                                span.clone(),
                            )
                            .with_label(format!("expected `{}`", declared_ty)),
                        );
                    }
                    declared_ty
                } else {
                    init_ty
                };
                self.define_var(VarSymbol {
                    name: name.clone(),
                    ty: final_ty,
                    is_mutable: true,
                    capability: Capability::Value,
                    region_origin: None,
                    span: span.clone(),
                });
                Type::Void
            }
            Stmt::Const(c) => {
                let (_, mut ty) = self.tcx.resolve_type_ast(&c.ty);
                let init_ty = self.infer_expr(&c.value);
                if ty == Type::Unknown {
                    ty = init_ty;
                }
                self.define_var(VarSymbol {
                    name: c.name.clone(),
                    ty,
                    is_mutable: false,
                    capability: Capability::Value,
                    region_origin: None,
                    span: c.span.clone(),
                });
                Type::Void
            }
            Stmt::Assign {
                target,
                op: _,
                value,
                span,
            } => {
                let target_ty = self.infer_expr(target);
                let val_ty = self.infer_expr(value);
                if !target_ty.is_compatible_with(&val_ty) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0101",
                        format!(
                            "cannot assign value of type `{}` to target of type `{}`",
                            val_ty, target_ty
                        ),
                        span.clone(),
                    ));
                }
                Type::Void
            }
            Stmt::Return { value, span } => {
                let val_ty = value
                    .as_ref()
                    .map(|e| self.infer_expr(e))
                    .unwrap_or(Type::Void);
                if !self.current_fn_ret.is_compatible_with(&val_ty) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0101",
                        format!(
                            "return type mismatch: expected `{}`, found `{}`",
                            self.current_fn_ret, val_ty
                        ),
                        span.clone(),
                    ));
                }
                Type::Never
            }
            Stmt::Break { span } | Stmt::Continue { span } => {
                if self.in_loop_depth == 0 {
                    self.diagnostics.push(Diagnostic::error(
                        "E0108",
                        "`break` or `continue` used outside of a loop",
                        span.clone(),
                    ));
                }
                Type::Never
            }
            Stmt::Defer { body, .. } => {
                let _ = self.check_block(body);
                Type::Void
            }
            Stmt::Region { name, body, span } => {
                self.active_regions.push(name.clone());
                self.push_scope();
                self.define_var(VarSymbol {
                    name: name.clone(),
                    ty: Type::RegionArena(name.clone()),
                    is_mutable: true,
                    capability: Capability::Write,
                    region_origin: Some(name.clone()),
                    span: span.clone(),
                });
                let _ = self.check_block(body);
                self.pop_scope();
                self.active_regions.pop();
                Type::Void
            }
            Stmt::Unsafe { body, .. } => {
                let prev = self.in_unsafe_block;
                self.in_unsafe_block = true;
                let _ = self.check_block(body);
                self.in_unsafe_block = prev;
                Type::Void
            }
            Stmt::While { cond, body, span } => {
                let cond_ty = self.infer_expr(cond);
                if !cond_ty.is_compatible_with(&Type::Bool) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0101",
                        format!("`while` condition must be `bool`, found `{}`", cond_ty),
                        span.clone(),
                    ));
                }
                self.in_loop_depth += 1;
                let _ = self.check_block(body);
                self.in_loop_depth -= 1;
                Type::Void
            }
            Stmt::For {
                var,
                iter,
                body,
                span,
            } => {
                let iter_ty = self.infer_expr(iter);
                let elem_ty = match iter_ty {
                    Type::Array(elem, _) | Type::Vec(elem) => *elem,
                    _ => Type::I64,
                };
                self.in_loop_depth += 1;
                self.push_scope();
                self.define_var(VarSymbol {
                    name: var.clone(),
                    ty: elem_ty,
                    is_mutable: false,
                    capability: Capability::Value,
                    region_origin: None,
                    span: span.clone(),
                });
                let _ = self.check_block(body);
                self.pop_scope();
                self.in_loop_depth -= 1;
                Type::Void
            }
            Stmt::Loop { body, .. } => {
                self.in_loop_depth += 1;
                let _ = self.check_block(body);
                self.in_loop_depth -= 1;
                Type::Void
            }
            Stmt::Expr { expr, .. } => self.infer_expr(expr),
        }
    }

    pub fn infer_expr(&mut self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::IntLit(_, suffix) => match suffix.as_deref() {
                Some("i8") => Type::I8,
                Some("i16") => Type::I16,
                Some("i32") => Type::I32,
                Some("i64") => Type::I64,
                Some("isize") => Type::Isize,
                Some("u8") => Type::U8,
                Some("u16") => Type::U16,
                Some("u32") => Type::U32,
                Some("u64") => Type::U64,
                Some("usize") => Type::Usize,
                _ => Type::I64,
            },
            ExprKind::FloatLit(_, suffix) => match suffix.as_deref() {
                Some("f32") => Type::F32,
                _ => Type::F64,
            },
            ExprKind::StringLit(_) => Type::String,
            ExprKind::CharLit(_) => Type::Char,
            ExprKind::BoolLit(_) => Type::Bool,
            ExprKind::Ident(name) => {
                if let Some(sym) = self.lookup_var(name) {
                    return sym.ty.clone();
                }
                if name == "None" {
                    return Type::Option(Box::new(Type::Unknown));
                }
                // Check if it's an enum unit variant like `Status::Idle`
                if let Some((enum_part, _)) = name.split_once("::") {
                    if self.tcx.known_enums.contains_key(enum_part) {
                        return Type::Enum(enum_part.to_string(), Vec::new());
                    }
                    return Type::Unknown;
                }
                for (ename, variants) in &self.tcx.known_enums {
                    if variants.iter().any(|(vn, _)| vn == name) {
                        return Type::Enum(ename.clone(), Vec::new());
                    }
                }
                if self.functions.contains_key(name) || self.tcx.known_structs.contains_key(name) {
                    return Type::Unknown;
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        "E0102",
                        format!("cannot find value `{}` in this scope", name),
                        expr.span.clone(),
                    )
                    .with_help("check the variable name or declare it with `let` or `var`"),
                );
                Type::Error
            }
            ExprKind::Path(segs) => {
                if let Some(first) = segs.first() {
                    if self.tcx.known_enums.contains_key(first) {
                        return Type::Enum(first.clone(), Vec::new());
                    }
                }
                Type::Unknown
            }
            ExprKind::Binary { op, left, right } => {
                let lt = self.infer_expr(left);
                let rt = self.infer_expr(right);
                match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        if !lt.is_compatible_with(&rt) {
                            self.diagnostics.push(Diagnostic::error(
                                "E0101",
                                format!("cannot compare `{}` with `{}`", lt, rt),
                                expr.span.clone(),
                            ));
                        }
                        Type::Bool
                    }
                    BinOp::And | BinOp::Or => Type::Bool,
                    BinOp::Add
                        if matches!(lt.strip_region(), Type::String | Type::Str)
                            || matches!(rt.strip_region(), Type::String | Type::Str) =>
                    {
                        Type::String
                    }
                    _ => {
                        if !lt.is_compatible_with(&rt) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0101",
                                    format!(
                                        "binary operation cannot be applied to `{}` and `{}`",
                                        lt, rt
                                    ),
                                    expr.span.clone(),
                                )
                                .with_help("use `as` for explicit numeric conversion"),
                            );
                        }
                        lt
                    }
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                let it = self.infer_expr(inner);
                match op {
                    UnaryOp::Not => Type::Bool,
                    UnaryOp::Share => Type::Shared(Box::new(it)),
                    UnaryOp::Spawn => Type::Task(Box::new(it)),
                    UnaryOp::Await => {
                        if let Type::Task(t) = it {
                            *t
                        } else {
                            it
                        }
                    }
                    _ => it,
                }
            }
            ExprKind::Cast { expr: inner, ty } => {
                let _ = self.infer_expr(inner);
                self.tcx.resolve_type_ast(ty).1
            }
            ExprKind::Call { callee, args } => {
                let mut callee_name = match &callee.kind {
                    ExprKind::Ident(n) => Some(n.clone()),
                    ExprKind::FieldAccess { base, field } => {
                        if let ExprKind::Ident(b) = &base.kind {
                            Some(format!("{}::{}", b, field))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                for arg in args {
                    let _ = self.infer_expr(&arg.expr);
                }

                if let Some(ref name) = callee_name {
                    if name == "Some" {
                        let inner_ty = args
                            .first()
                            .map(|a| self.infer_expr(&a.expr))
                            .unwrap_or(Type::Unknown);
                        return Type::Option(Box::new(inner_ty));
                    }
                    if name == "Ok" {
                        let ok_ty = args
                            .first()
                            .map(|a| self.infer_expr(&a.expr))
                            .unwrap_or(Type::Unknown);
                        return Type::Result(Box::new(ok_ty), Box::new(Type::Unknown));
                    }
                    if name == "Err" {
                        let err_ty = args
                            .first()
                            .map(|a| self.infer_expr(&a.expr))
                            .unwrap_or(Type::String);
                        return Type::Result(Box::new(Type::Unknown), Box::new(err_ty));
                    }
                    if name == "share" {
                        let inner_ty = args
                            .first()
                            .map(|a| self.infer_expr(&a.expr))
                            .unwrap_or(Type::Unknown);
                        return Type::Shared(Box::new(inner_ty));
                    }
                    if let Some((enum_part, _)) = name.split_once("::") {
                        if self.tcx.known_enums.contains_key(enum_part) {
                            return Type::Enum(enum_part.to_string(), Vec::new());
                        }
                    }
                    if let Some(fn_sym) = self.functions.get(name).cloned() {
                        if fn_sym.is_extern && !self.in_unsafe_block {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0601",
                                    format!(
                                        "call to extern function `{}` requires an `unsafe` block",
                                        name
                                    ),
                                    expr.span.clone(),
                                )
                                .with_help("wrap the call in `unsafe { ... }`"),
                            );
                        }
                        if fn_sym.params.len() != args.len()
                            && name != "print"
                            && name != "println"
                            && name != "format"
                        {
                            self.diagnostics.push(Diagnostic::error(
                                "E0103",
                                format!(
                                    "function `{}` expects {} argument(s), found {}",
                                    name,
                                    fn_sym.params.len(),
                                    args.len()
                                ),
                                expr.span.clone(),
                            ));
                        }
                        if let Type::GenericParam(_) = fn_sym.ret_type {
                            if let Some(first_arg) = args.first() {
                                return self.infer_expr(&first_arg.expr);
                            }
                        }
                        if name == "promote" {
                            if let Some(first_arg) = args.first() {
                                let arg_ty = self.infer_expr(&first_arg.expr);
                                return arg_ty.strip_region().clone();
                            }
                        }
                        return fn_sym.ret_type;
                    } else if let Some(var) = self.lookup_var(name).cloned() {
                        match &var.ty {
                            Type::Function(p_tys, ret_ty) => {
                                if p_tys.len() != args.len() {
                                    self.diagnostics.push(Diagnostic::error(
                                        "E0103",
                                        format!(
                                            "callable `{}` expects {} argument(s), found {}",
                                            name,
                                            p_tys.len(),
                                            args.len()
                                        ),
                                        expr.span.clone(),
                                    ));
                                }
                                return (*ret_ty).as_ref().clone();
                            }
                            _ => return Type::I64,
                        }
                    } else if !name.contains("::") {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0102",
                                format!("undefined function `{}`", name),
                                expr.span.clone(),
                            )
                            .with_help("check the function name or define it with `fn`"),
                        );
                    }
                }
                let _ = callee_name.take();
                Type::Unknown
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                // Check if receiver is a type name like `Vec.new()`, `Driver.new()`, `Status.Running(1)`
                if let ExprKind::Ident(type_or_var) = &receiver.kind {
                    if self.lookup_var(type_or_var).is_none() {
                        for a in args {
                            let _ = self.infer_expr(&a.expr);
                        }
                        if self.tcx.known_enums.contains_key(type_or_var) {
                            return Type::Enum(type_or_var.clone(), Vec::new());
                        }
                        if let Some(m_sym) = self
                            .methods
                            .get(&(type_or_var.clone(), method.clone()))
                            .cloned()
                        {
                            return m_sym.ret_type;
                        }
                        if type_or_var == "HashMap" || type_or_var.starts_with("HashMap$") {
                            let (k, v) = if type_or_var.starts_with("HashMap$") {
                                let parts: Vec<&str> = type_or_var.split('$').collect();
                                let k = parts.get(1).map(|t| match *t {
                                    "String" | "str" => Type::String,
                                    "f32" | "f64" => Type::F64,
                                    "bool" => Type::Bool,
                                    "char" => Type::Char,
                                    _ => Type::I64,
                                }).unwrap_or(Type::Unknown);
                                let v = parts.get(2).map(|t| match *t {
                                    "String" | "str" => Type::String,
                                    "f32" | "f64" => Type::F64,
                                    "bool" => Type::Bool,
                                    "char" => Type::Char,
                                    _ => Type::I64,
                                }).unwrap_or(Type::Unknown);
                                (k, v)
                            } else {
                                (Type::Unknown, Type::Unknown)
                            };
                            return Type::HashMap(Box::new(k), Box::new(v));
                        }
                        match type_or_var.as_str() {
                            "Vec" => return Type::Vec(Box::new(Type::Unknown)),
                            "HashSet" => return Type::HashSet(Box::new(Type::Unknown)),
                            "Mutex" => {
                                let inner = args
                                    .first()
                                    .map(|a| self.infer_expr(&a.expr))
                                    .unwrap_or(Type::I64);
                                return Type::Mutex(Box::new(inner));
                            }
                            "Channel" => return Type::Channel(Box::new(Type::Unknown)),
                            _ => return Type::Struct(type_or_var.clone(), Vec::new()),
                        }
                    }
                }

                let recv_ty = self.infer_expr(receiver);
                for a in args {
                    let _ = self.infer_expr(&a.expr);
                }

                match recv_ty.strip_region() {
                    Type::RegionArena(rname) if method == "alloc" => {
                        let inner_ty = args
                            .first()
                            .map(|a| self.infer_expr(&a.expr))
                            .unwrap_or(Type::I64);
                        return Type::RegionBound {
                            region: rname.clone(),
                            inner: Box::new(inner_ty),
                        };
                    }
                    Type::String | Type::Str => match method.as_str() {
                        "len" | "byte_len" | "char_count" | "to_i64" => return Type::I64,
                        "is_empty" | "contains" | "starts_with" | "ends_with" => {
                            return Type::Bool
                        }
                        "trim" | "to_upper" | "to_lower" | "slice_bytes" | "slice" | "clone" => return Type::String,
                        _ => {}
                    },
                    Type::Array(elem, _) | Type::Vec(elem) => match method.as_str() {
                        "len" | "capacity" => return Type::I64,
                        "is_empty" | "contains" => return Type::Bool,
                        "push" | "clear" => return Type::Void,
                        "pop" | "get" => return (**elem).clone(),
                        _ => {}
                    },
                    Type::Struct(sname, _) => {
                        if let Some(m_sym) =
                            self.methods.get(&(sname.clone(), method.clone())).cloned()
                        {
                            return m_sym.ret_type;
                        }
                    }
                    Type::Option(inner) => match method.as_str() {
                        "is_some" | "is_none" => return Type::Bool,
                        "unwrap" | "unwrap_or" => return (**inner).clone(),
                        _ => {}
                    },
                    Type::Result(ok, _) => match method.as_str() {
                        "is_ok" | "is_err" => return Type::Bool,
                        "unwrap" | "unwrap_or" => return (**ok).clone(),
                        _ => {}
                    },
                    Type::Shared(inner) => match method.as_str() {
                        "downgrade" => return Type::Weak(inner.clone()),
                        "clone" => return Type::Shared(inner.clone()),
                        _ => {}
                    },
                    Type::Weak(inner) => {
                        if method == "upgrade" {
                            return Type::Option(Box::new(Type::Shared(inner.clone())));
                        }
                    }
                    Type::Channel(inner) => match method.as_str() {
                        "send" => return Type::Void,
                        "recv" => return Type::Option(inner.clone()),
                        "close" => return Type::Void,
                        _ => {}
                    },
                    Type::Task(inner) => {
                        if method == "join" {
                            return (**inner).clone();
                        }
                    }
                    _ => {
                        if method == "recv" {
                            return Type::Option(Box::new(Type::Unknown));
                        }
                        if method == "send" || method == "close" {
                            return Type::Void;
                        }
                        if method == "join" {
                            return Type::I64;
                        }
                    }
                }
                Type::Unknown
            }
            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(bname) = &base.kind {
                    if self.lookup_var(bname).is_none() && self.tcx.known_enums.contains_key(bname)
                    {
                        return Type::Enum(bname.clone(), Vec::new());
                    }
                }
                let base_ty = self.infer_expr(base);
                let clean_ty = match base_ty.strip_region() {
                    Type::Shared(inner) => inner.as_ref(),
                    other => other,
                };
                if let Type::Channel(inner) = clean_ty {
                    if field == "sender" || field == "receiver" {
                        return Type::Channel(inner.clone());
                    }
                }
                if let Type::Struct(sname, _) = clean_ty {
                    if let Some(fields) = self.tcx.known_structs.get(sname) {
                        for (fname, fty) in fields {
                            if fname == field {
                                return fty.clone();
                            }
                        }
                        self.diagnostics.push(Diagnostic::error(
                            "E0109",
                            format!("struct `{}` has no field named `{}`", sname, field),
                            expr.span.clone(),
                        ));
                        return Type::Error;
                    }
                }
                Type::Unknown
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.infer_expr(base);
                let _ = self.infer_expr(index);
                match base_ty.strip_region() {
                    Type::Array(elem, _) | Type::Vec(elem) => (**elem).clone(),
                    _ => Type::I64,
                }
            }
            ExprKind::StructInit { name, fields } => {
                let mut field_tys = Vec::new();
                for (_, fexpr) in fields {
                    field_tys.push(self.infer_expr(fexpr));
                }
                if let Some(expected_fields) = self.tcx.known_structs.get(name).cloned() {
                    for (fname, fexpr) in fields {
                        let fty = self.infer_expr(fexpr);
                        if let Some((_, exp_ty)) =
                            expected_fields.iter().find(|(en, _)| en == fname)
                        {
                            if !exp_ty.is_compatible_with(&fty) {
                                self.diagnostics.push(Diagnostic::error(
                                    "E0101",
                                    format!(
                                        "mismatched type for field `{}` of `{}`: expected `{}`, found `{}`",
                                        fname, name, exp_ty, fty
                                    ),
                                    fexpr.span.clone(),
                                ));
                            }
                        }
                    }
                }
                Type::Struct(name.clone(), Vec::new())
            }
            ExprKind::ArrayLit(elems) => {
                let elem_ty = elems
                    .first()
                    .map(|e| self.infer_expr(e))
                    .unwrap_or(Type::I64);
                for e in elems.iter().skip(1) {
                    let _ = self.infer_expr(e);
                }
                Type::Array(Box::new(elem_ty), Some(elems.len()))
            }
            ExprKind::Range { start, end, .. } => {
                let s = self.infer_expr(start);
                let _ = self.infer_expr(end);
                Type::Array(Box::new(s), None)
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                let cond_ty = self.infer_expr(cond);
                if !cond_ty.is_compatible_with(&Type::Bool) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0101",
                        format!("`if` condition must be `bool`, found `{}`", cond_ty),
                        cond.span.clone(),
                    ));
                }
                let then_ty = self.check_block(then_block);
                if let Some(eb) = else_branch {
                    let _ = self.infer_expr(eb);
                    then_ty
                } else {
                    Type::Void
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrut_ty = self.infer_expr(scrutinee);
                let mut res_ty = Type::Void;
                for arm in arms {
                    self.push_scope();
                    self.bind_pattern(&arm.pattern, &scrut_ty, &arm.span);
                    let arm_ty = self.check_block(&arm.body);
                    if res_ty == Type::Void {
                        res_ty = arm_ty;
                    }
                    self.pop_scope();
                }
                res_ty
            }
            ExprKind::Closure {
                params,
                ret_type,
                body,
            } => {
                let (_, declared_ret) = self.tcx.resolve_type_ast(ret_type);
                let prev_ret = self.current_fn_ret.clone();
                self.current_fn_ret = declared_ret.clone();
                self.push_scope();
                let mut param_tys = Vec::new();
                for p in params {
                    let (_, pt) = self.tcx.resolve_type_ast(&p.ty);
                    param_tys.push((p.capability, pt.clone()));
                    self.define_var(VarSymbol {
                        name: p.name.clone(),
                        ty: pt,
                        is_mutable: p.is_mutable,
                        capability: p.capability,
                        region_origin: None,
                        span: p.span.clone(),
                    });
                }
                let body_ty = self.check_block(body);
                self.pop_scope();
                self.current_fn_ret = prev_ret;
                let final_ret = if declared_ret != Type::Unknown {
                    declared_ret
                } else {
                    body_ty
                };
                Type::Function(param_tys, Box::new(final_ret))
            }
            ExprKind::Block(b) => self.check_block(b),
            ExprKind::TryProp(inner) => {
                let inner_ty = self.infer_expr(inner);
                match inner_ty {
                    Type::Result(ok, _) => *ok,
                    Type::Option(some) => *some,
                    other => other,
                }
            }
        }
    }

    fn bind_pattern(&mut self, pat: &Pattern, scrut_ty: &Type, span: &Span) {
        match pat {
            Pattern::Ident(name) => {
                self.define_var(VarSymbol {
                    name: name.clone(),
                    ty: scrut_ty.clone(),
                    is_mutable: false,
                    capability: Capability::Read,
                    region_origin: None,
                    span: span.clone(),
                });
            }
            Pattern::Variant {
                enum_name,
                variant_name,
                bindings,
            } => {
                let mut payload_tys = Vec::new();
                if variant_name == "Some" {
                    if let Type::Option(inner) = scrut_ty {
                        payload_tys.push((**inner).clone());
                    }
                } else if variant_name == "Ok" {
                    if let Type::Result(ok, _) = scrut_ty {
                        payload_tys.push((**ok).clone());
                    }
                } else if variant_name == "Err" {
                    if let Type::Result(_, err) = scrut_ty {
                        payload_tys.push((**err).clone());
                    }
                } else {
                    let target_enum = enum_name.clone().or_else(|| {
                        if let Type::Enum(en, _) = scrut_ty {
                            Some(en.clone())
                        } else {
                            None
                        }
                    });
                    if let Some(en) = target_enum {
                        if let Some(vars) = self.tcx.known_enums.get(&en) {
                            if let Some((_, pts)) = vars.iter().find(|(vn, _)| vn == variant_name) {
                                payload_tys = pts.clone();
                            }
                        }
                    }
                }
                for (i, b) in bindings.iter().enumerate() {
                    let bty = payload_tys.get(i).cloned().unwrap_or(Type::I64);
                    self.define_var(VarSymbol {
                        name: b.clone(),
                        ty: bty,
                        is_mutable: false,
                        capability: Capability::Read,
                        region_origin: None,
                        span: span.clone(),
                    });
                }
            }
            _ => {}
        }
    }
}
