use crate::compiler::ast::*;
use crate::compiler::diagnostics::{Diagnostic, DiagnosticBag, Span};
use crate::compiler::semantic::FnSymbol;
use crate::compiler::types::{Type, TypeContext};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmaTier {
    LocalStack,
    ManagedHeap,
    RegionArena(String),
    SharedImmutable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipState {
    Owned,
    BorrowedRead,
    BorrowedWrite,
    Moved { moved_at: Span },
    SharedFrozen,
}

#[derive(Debug, Clone)]
pub struct AmaVarBinding {
    pub name: String,
    pub ty: Type,
    pub tier: AmaTier,
    pub state: OwnershipState,
    pub capability: Capability,
    pub is_mutable: bool,
    pub scope_depth: usize,
    pub region_tag: Option<String>,
    pub moved_fields: HashMap<String, Span>,
    pub decl_span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct AmaReport {
    pub variable_tiers: HashMap<String, AmaTier>,
    pub ctfp_drops: Vec<(String, usize)>, // (variable_name, scope_depth)
    pub active_regions: Vec<String>,
}

pub struct AmaAnalyzer {
    pub tcx: TypeContext,
    pub functions: HashMap<String, FnSymbol>,
    pub scopes: Vec<Vec<AmaVarBinding>>,
    pub active_regions: Vec<(String, usize)>, // (region_name, scope_depth)
    pub report: AmaReport,
    pub diagnostics: DiagnosticBag,
}

impl AmaAnalyzer {
    pub fn new(tcx: TypeContext, functions: HashMap<String, FnSymbol>) -> Self {
        Self {
            tcx,
            functions,
            scopes: vec![Vec::new()],
            active_regions: Vec::new(),
            report: AmaReport::default(),
            diagnostics: DiagnosticBag::new(),
        }
    }

    fn current_depth(&self) -> usize {
        self.scopes.len().saturating_sub(1)
    }

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        let depth = self.current_depth();
        if let Some(vars) = self.scopes.pop() {
            // LIFO deterministic drop placement (CTFP)
            for v in vars.into_iter().rev() {
                if v.state == OwnershipState::Owned
                    && !v.ty.is_copy()
                    && matches!(v.tier, AmaTier::ManagedHeap | AmaTier::SharedImmutable)
                {
                    self.report.ctfp_drops.push((v.name, depth));
                }
            }
        }
    }

    fn declare_var(&mut self, binding: AmaVarBinding) {
        self.report
            .variable_tiers
            .insert(binding.name.clone(), binding.tier.clone());
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(binding);
        }
    }

    fn find_var(&self, name: &str) -> Option<&AmaVarBinding> {
        for scope in self.scopes.iter().rev() {
            for b in scope.iter().rev() {
                if b.name == name {
                    return Some(b);
                }
            }
        }
        None
    }

    fn find_var_mut(&mut self, name: &str) -> Option<&mut AmaVarBinding> {
        for scope in self.scopes.iter_mut().rev() {
            for b in scope.iter_mut().rev() {
                if b.name == name {
                    return Some(b);
                }
            }
        }
        None
    }

    fn extract_access_path(expr: &Expr) -> Option<(String, Vec<String>)> {
        match &expr.kind {
            ExprKind::Ident(name) => Some((name.clone(), Vec::new())),
            ExprKind::FieldAccess { base, field } => {
                let (root, mut path) = Self::extract_access_path(base)?;
                path.push(field.clone());
                Some((root, path))
            }
            ExprKind::Unary {
                op: UnaryOp::Read | UnaryOp::Write | UnaryOp::Take,
                expr: inner,
            } => Self::extract_access_path(inner),
            _ => None,
        }
    }

    fn paths_overlap(p1: &(String, Vec<String>), p2: &(String, Vec<String>)) -> bool {
        if p1.0 != p2.0 {
            return false;
        }
        let min_len = p1.1.len().min(p2.1.len());
        p1.1[..min_len] == p2.1[..min_len]
    }

    pub fn analyze_program(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(f) => self.analyze_fn(f),
                Item::Impl(imp) => {
                    for m in &imp.methods {
                        self.analyze_fn(m);
                    }
                }
                _ => {}
            }
        }
    }

    fn analyze_fn(&mut self, f: &FnDecl) {
        self.push_scope();
        let depth = self.current_depth();
        for p in &f.params {
            let (cap_t, ty) = self.tcx.resolve_type_ast(&p.ty);
            let cap = if p.capability != Capability::Value {
                p.capability
            } else {
                cap_t
            };
            let state = match cap {
                Capability::Read => OwnershipState::BorrowedRead,
                Capability::Write => OwnershipState::BorrowedWrite,
                Capability::Take | Capability::Value => OwnershipState::Owned,
            };
            let tier = if ty.is_copy() {
                AmaTier::LocalStack
            } else {
                AmaTier::ManagedHeap
            };
            self.declare_var(AmaVarBinding {
                name: p.name.clone(),
                ty,
                tier,
                state,
                capability: cap,
                is_mutable: p.is_mutable || cap == Capability::Write,
                scope_depth: depth,
                region_tag: None,
                moved_fields: HashMap::new(),
                decl_span: p.span.clone(),
            });
        }

        self.analyze_block(&f.body);
        self.pop_scope();
    }

    fn analyze_block(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.stmts {
            self.analyze_stmt(stmt);
        }
        self.pop_scope();
    }

    fn infer_expr_region(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => self.find_var(name).and_then(|v| v.region_tag.clone()),
            ExprKind::MethodCall {
                receiver, method, ..
            } => {
                if method == "alloc" {
                    if let ExprKind::Ident(rname) = &receiver.kind {
                        if self.active_regions.iter().any(|(r, _)| r == rname) {
                            return Some(rname.clone());
                        }
                    }
                }
                self.infer_expr_region(receiver)
            }
            ExprKind::FieldAccess { base, .. } => self.infer_expr_region(base),
            ExprKind::Unary { expr: inner, .. } => self.infer_expr_region(inner),
            ExprKind::Call { args, .. } => {
                for a in args {
                    if let Some(r) = self.infer_expr_region(&a.expr) {
                        return Some(r);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn infer_expr_tier_and_type(&self, expr: &Expr, declared_ty: &Type) -> (AmaTier, Type, bool) {
        if let Some(reg) = self.infer_expr_region(expr) {
            return (AmaTier::RegionArena(reg), declared_ty.clone(), false);
        }
        match &expr.kind {
            ExprKind::Unary {
                op: UnaryOp::Share, ..
            } => (AmaTier::SharedImmutable, declared_ty.clone(), true),
            ExprKind::Call { callee, .. } => {
                if let ExprKind::Ident(n) = &callee.kind {
                    if n == "share" {
                        return (AmaTier::SharedImmutable, declared_ty.clone(), true);
                    }
                }
                let tier = if declared_ty.is_copy() {
                    AmaTier::LocalStack
                } else {
                    AmaTier::ManagedHeap
                };
                (tier, declared_ty.clone(), false)
            }
            ExprKind::StructInit { name, .. } => {
                let st_ty = Type::Struct(name.clone(), Vec::new());
                // Escape Analysis heuristic: local struct init starts on LocalStack unless it holds heap strings/vecs or escapes
                let has_heap_field = self
                    .tcx
                    .known_structs
                    .get(name)
                    .map(|fs| fs.iter().any(|(_, ft)| !ft.is_copy()))
                    .unwrap_or(false);
                let tier = if has_heap_field {
                    AmaTier::ManagedHeap
                } else {
                    AmaTier::LocalStack
                };
                (tier, st_ty, false)
            }
            ExprKind::StringLit(_) => (AmaTier::ManagedHeap, Type::String, false),
            ExprKind::IntLit(_, _) => (AmaTier::LocalStack, Type::I64, false),
            ExprKind::FloatLit(_, _) => (AmaTier::LocalStack, Type::F64, false),
            ExprKind::BoolLit(_) => (AmaTier::LocalStack, Type::Bool, false),
            ExprKind::CharLit(_) => (AmaTier::LocalStack, Type::Char, false),
            ExprKind::Ident(name) => {
                if let Some(v) = self.find_var(name) {
                    (
                        v.tier.clone(),
                        v.ty.clone(),
                        v.state == OwnershipState::SharedFrozen,
                    )
                } else {
                    (AmaTier::LocalStack, declared_ty.clone(), false)
                }
            }
            _ => {
                let tier = if declared_ty.is_copy() {
                    AmaTier::LocalStack
                } else {
                    AmaTier::ManagedHeap
                };
                (tier, declared_ty.clone(), false)
            }
        }
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                ty,
                init,
                span,
            } => {
                let (cap_t, resolved_ty) = self.tcx.resolve_type_ast(ty);
                self.check_expr_as_rvalue(init);
                let region_tag = self.infer_expr_region(init);
                let (tier, inferred_ty, is_shared) =
                    self.infer_expr_tier_and_type(init, &resolved_ty);
                let final_ty = if resolved_ty != Type::Unknown {
                    resolved_ty
                } else {
                    inferred_ty
                };
                let state = if is_shared || matches!(final_ty, Type::Shared(_)) {
                    OwnershipState::SharedFrozen
                } else if cap_t == Capability::Read {
                    OwnershipState::BorrowedRead
                } else {
                    OwnershipState::Owned
                };
                let depth = self.current_depth();
                self.declare_var(AmaVarBinding {
                    name: name.clone(),
                    ty: final_ty,
                    tier,
                    state,
                    capability: cap_t,
                    is_mutable: false,
                    scope_depth: depth,
                    region_tag,
                    moved_fields: HashMap::new(),
                    decl_span: span.clone(),
                });
            }
            Stmt::Var {
                name,
                ty,
                init,
                span,
            } => {
                let (cap_t, resolved_ty) = self.tcx.resolve_type_ast(ty);
                self.check_expr_as_rvalue(init);
                let region_tag = self.infer_expr_region(init);
                let (tier, inferred_ty, is_shared) =
                    self.infer_expr_tier_and_type(init, &resolved_ty);
                let final_ty = if resolved_ty != Type::Unknown {
                    resolved_ty
                } else {
                    inferred_ty
                };
                let state = if is_shared || matches!(final_ty, Type::Shared(_)) {
                    OwnershipState::SharedFrozen
                } else {
                    OwnershipState::Owned
                };
                let depth = self.current_depth();
                self.declare_var(AmaVarBinding {
                    name: name.clone(),
                    ty: final_ty,
                    tier,
                    state,
                    capability: cap_t,
                    is_mutable: true,
                    scope_depth: depth,
                    region_tag,
                    moved_fields: HashMap::new(),
                    decl_span: span.clone(),
                });
            }
            Stmt::Const(c) => {
                self.check_expr(
                    &c.value,
                    Capability::Read,
                );
            }
            Stmt::Assign {
                target,
                op: _,
                value,
                span,
            } => {
                self.check_expr(value, Capability::Read);
                let val_region = self.infer_expr_region(value);

                if let Some((root, field_path)) = Self::extract_access_path(target) {
                    if let Some(binding) = self.find_var(&root).cloned() {
                        // 1. Check if root was moved
                        if let OwnershipState::Moved { ref moved_at } = binding.state {
                            if !field_path.is_empty() {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        "E0201",
                                        format!("use of moved value `{}`", root),
                                        span.clone(),
                                    )
                                    .with_note(format!(
                                        "value was previously moved at line {}",
                                        moved_at.line
                                    ))
                                    .with_help(
                                        "pass by `read` or `write` capability instead of `take` if ownership is still needed",
                                    ),
                                );
                                return;
                            }
                        }

                        // 2. Check if target is `read` capability or `SharedFrozen` -> E0042!
                        if binding.capability == Capability::Read
                            || binding.state == OwnershipState::BorrowedRead
                            || binding.state == OwnershipState::SharedFrozen
                        {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0042",
                                    "cannot modify a read-only value",
                                    span.clone(),
                                )
                                .with_label("cannot modify through read access")
                                .with_help("request exclusive write access to this value"),
                            );
                            return;
                        }

                        // 3. Check if target is an immutable `let` binding
                        if !binding.is_mutable && binding.capability != Capability::Write {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0042",
                                    "cannot modify a read-only value",
                                    span.clone(),
                                )
                                .with_label(format!(
                                    "`{}` is declared as immutable `let`",
                                    root
                                ))
                                .with_help(format!(
                                    "declare `{}` as mutable using `var {}` or request `write` access",
                                    root, root
                                )),
                            );
                            return;
                        }

                        // 4. Region Non-Escape Check (E0303):
                        // Cannot store a region-allocated non-copy value into a variable declared in an outer scope!
                        if !binding.ty.is_copy() {
                            if let Some(ref reg_name) = val_region {
                                if let Some((_, reg_depth)) =
                                    self.active_regions.iter().find(|(r, _)| r == reg_name)
                                {
                                    if binding.scope_depth < *reg_depth {
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                "E0303",
                                                format!(
                                                    "value allocated in region `{}` escapes to outer variable `{}`",
                                                    reg_name, root
                                                ),
                                                span.clone(),
                                            )
                                            .with_label(format!(
                                                "region `{}` is destroyed at the end of its block, before `{}` goes out of scope",
                                                reg_name, root
                                            ))
                                            .with_help(
                                                "clone or promote the data out of the region before storing it in an outer binding",
                                            ),
                                        );
                                    }
                                }
                            }
                        }

                        // Re-initializing a moved root variable makes it Owned again
                        if field_path.is_empty() {
                            if let Some(b_mut) = self.find_var_mut(&root) {
                                b_mut.state = OwnershipState::Owned;
                                b_mut.moved_fields.clear();
                            }
                        }
                    }
                } else {
                    self.check_expr(target, Capability::Write);
                }
            }
            Stmt::Return { value, span } => {
                if let Some(val) = value {
                    self.check_expr(val, Capability::Read);
                    if let Some(reg_name) = self.infer_expr_region(val) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0303",
                                format!(
                                    "cannot return value bound to local region `{}` from function",
                                    reg_name
                                ),
                                span.clone(),
                            )
                            .with_label("value would be dangling after region destruction")
                            .with_help("allocate returned values in Managed Ownership instead of a local `region`"),
                        );
                    }
                }
            }
            Stmt::Region { name, body, span } => {
                let depth = self.current_depth() + 1;
                self.active_regions.push((name.clone(), depth));
                self.report.active_regions.push(name.clone());
                self.push_scope();
                self.declare_var(AmaVarBinding {
                    name: name.clone(),
                    ty: Type::RegionArena(name.clone()),
                    tier: AmaTier::RegionArena(name.clone()),
                    state: OwnershipState::Owned,
                    capability: Capability::Write,
                    is_mutable: true,
                    scope_depth: depth,
                    region_tag: Some(name.clone()),
                    moved_fields: HashMap::new(),
                    decl_span: span.clone(),
                });
                for s in &body.stmts {
                    self.analyze_stmt(s);
                }
                self.pop_scope();
                self.active_regions.pop();
            }
            Stmt::Defer { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Loop { body, .. } => {
                self.analyze_block(body);
            }
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond, Capability::Read);
                self.analyze_block(body);
            }
            Stmt::For {
                var,
                iter,
                body,
                span,
            } => {
                self.check_expr(iter, Capability::Read);
                self.push_scope();
                let depth = self.current_depth();
                self.declare_var(AmaVarBinding {
                    name: var.clone(),
                    ty: Type::I64,
                    tier: AmaTier::LocalStack,
                    state: OwnershipState::Owned,
                    capability: Capability::Value,
                    is_mutable: false,
                    scope_depth: depth,
                    region_tag: None,
                    moved_fields: HashMap::new(),
                    decl_span: span.clone(),
                });
                for s in &body.stmts {
                    self.analyze_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr, Capability::Read);
            }
        }
    }

    fn check_expr_as_rvalue(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Unary {
                op: UnaryOp::Take | UnaryOp::Share,
                expr: inner,
            } => {
                self.consume_value(inner, &expr.span);
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Ident(n) = &callee.kind {
                    if n == "share" {
                        if let Some(first) = args.first() {
                            self.consume_value(&first.expr, &first.expr.span);
                            return;
                        }
                    }
                }
                self.check_expr(expr, Capability::Read);
            }
            _ => {
                self.check_expr(expr, Capability::Read);
            }
        }
    }

    fn consume_value(&mut self, expr: &Expr, span: &Span) {
        if let Some((root, field_path)) = Self::extract_access_path(expr) {
            if let Some(binding) = self.find_var(&root).cloned() {
                if let OwnershipState::Moved { ref moved_at } = binding.state {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E0201",
                            format!("use of moved value `{}`", root),
                            span.clone(),
                        )
                        .with_note(format!(
                            "`{}` was previously moved at line {}",
                            root, moved_at.line
                        ))
                        .with_help("clone the value or use `read`/`write` access capabilities"),
                    );
                    return;
                }
                if !field_path.is_empty() {
                    if let Some(f_moved) = binding.moved_fields.get(&field_path[0]) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0201",
                                format!("use of moved field `{}.{}`", root, field_path[0]),
                                span.clone(),
                            )
                            .with_note(format!(
                                "field was previously moved at line {}",
                                f_moved.line
                            )),
                        );
                        return;
                    }
                }
                if binding.capability == Capability::Read
                    || binding.capability == Capability::Write
                    || matches!(
                        binding.state,
                        OwnershipState::BorrowedRead | OwnershipState::BorrowedWrite
                    )
                {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E0203",
                            format!(
                                "cannot move (`take`) out of borrowed parameter `{}`",
                                root
                            ),
                            span.clone(),
                        )
                        .with_label("borrowed via `read` or `write` capability, not owned")
                        .with_help(format!(
                            "change parameter `{}` to `take` if ownership transfer is intended",
                            root
                        )),
                    );
                    return;
                }

                if !binding.ty.is_copy() {
                    if let Some(b_mut) = self.find_var_mut(&root) {
                        if field_path.is_empty() {
                            b_mut.state = OwnershipState::Moved {
                                moved_at: span.clone(),
                            };
                        } else {
                            b_mut
                                .moved_fields
                                .insert(field_path[0].clone(), span.clone());
                        }
                    }
                }
            }
        } else {
            self.check_expr(expr, Capability::Read);
        }
    }

    fn check_expr(&mut self, expr: &Expr, required_cap: Capability) {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some(binding) = self.find_var(name).cloned() {
                    if let OwnershipState::Moved { ref moved_at } = binding.state {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0201",
                                format!("use of moved value `{}`", name),
                                expr.span.clone(),
                            )
                            .with_label("value used here after move")
                            .with_note(format!(
                                "`{}` was moved at line {}",
                                name, moved_at.line
                            ))
                            .with_help(
                                "use `read` or `write` capability instead of `take` if you need to use the value again",
                            ),
                        );
                        return;
                    }
                    if !binding.moved_fields.is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0202",
                                format!(
                                    "use of partially moved struct `{}` as a whole",
                                    name
                                ),
                                expr.span.clone(),
                            )
                            .with_help("reinitialize moved fields or access only remaining unmoved fields"),
                        );
                    }
                    if required_cap == Capability::Write {
                        if binding.capability == Capability::Read
                            || binding.state == OwnershipState::BorrowedRead
                            || binding.state == OwnershipState::SharedFrozen
                            || (!binding.is_mutable && binding.capability != Capability::Write)
                        {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0042",
                                    "cannot modify a read-only value",
                                    expr.span.clone(),
                                )
                                .with_label("cannot acquire `write` capability on read-only binding")
                                .with_help("declare the binding with `var` or `write` capability"),
                            );
                        }
                    } else if required_cap == Capability::Take {
                        self.consume_value(expr, &expr.span);
                    }
                }
            }
            ExprKind::FieldAccess { base, field } => {
                if let Some((root, _)) = Self::extract_access_path(base) {
                    if let Some(binding) = self.find_var(&root).cloned() {
                        if let OwnershipState::Moved { ref moved_at } = binding.state {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0201",
                                    format!("use of moved value `{}.{}`", root, field),
                                    expr.span.clone(),
                                )
                                .with_note(format!("`{}` was moved at line {}", root, moved_at.line)),
                            );
                            return;
                        }
                        if let Some(moved_sp) = binding.moved_fields.get(field) {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0201",
                                    format!("use of moved field `{}.{}`", root, field),
                                    expr.span.clone(),
                                )
                                .with_note(format!(
                                    "field `{}` was moved at line {}",
                                    field, moved_sp.line
                                )),
                            );
                            return;
                        }
                        if required_cap == Capability::Write
                            && (binding.capability == Capability::Read
                                || binding.state == OwnershipState::BorrowedRead
                                || binding.state == OwnershipState::SharedFrozen
                                || (!binding.is_mutable && binding.capability != Capability::Write))
                        {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "E0042",
                                    "cannot modify a read-only value",
                                    expr.span.clone(),
                                )
                                .with_label("cannot modify through read access")
                                .with_help("request exclusive write access to this value"),
                            );
                        }
                    }
                } else {
                    self.check_expr(base, required_cap);
                }
            }
            ExprKind::Unary { op, expr: inner } => match op {
                UnaryOp::Write => self.check_expr(inner, Capability::Write),
                UnaryOp::Take | UnaryOp::Share => self.consume_value(inner, &expr.span),
                UnaryOp::Spawn => {
                    // Thread safety check: spawned expression cannot pass a `write` capability or region-bound variable
                    if let Some(reg) = self.infer_expr_region(inner) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0401",
                                format!(
                                    "cannot capture value from local region `{}` in `spawn` task",
                                    reg
                                ),
                                expr.span.clone(),
                            )
                            .with_help("use `share(...)` or pass owned data via `take`"),
                        );
                    }
                    self.check_expr(inner, Capability::Read);
                }
                _ => self.check_expr(inner, Capability::Read),
            },
            ExprKind::Binary { left, right, .. } => {
                self.check_expr(left, Capability::Read);
                self.check_expr(right, Capability::Read);
            }
            ExprKind::Call { callee, args } => {
                let callee_name = match &callee.kind {
                    ExprKind::Ident(n) => Some(n.clone()),
                    _ => None,
                };
                let fn_sym = callee_name
                    .as_ref()
                    .and_then(|n| self.functions.get(n).cloned());

                self.check_call_arguments(args, fn_sym.as_ref());
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                // Check mutating methods on Vec / HashMap / Struct
                let is_mutating_method = matches!(
                    method.as_str(),
                    "push" | "pop" | "insert" | "remove" | "clear" | "deposit" | "withdraw"
                );
                let recv_cap = if is_mutating_method {
                    Capability::Write
                } else {
                    Capability::Read
                };
                self.check_expr(receiver, recv_cap);

                if method == "send" {
                    // Channel.send consumes ownership
                    if let Some(first) = args.first() {
                        self.consume_value(&first.expr, &first.expr.span);
                        return;
                    }
                }

                // If method mutates receiver, verify exclusivity against all call arguments
                if recv_cap == Capability::Write {
                    if let Some(rpath) = Self::extract_access_path(receiver) {
                        for arg in args {
                            if let Some(apath) = Self::extract_access_path(&arg.expr) {
                                if Self::paths_overlap(&rpath, &apath) {
                                    let path_str = if rpath.1.is_empty() {
                                        rpath.0.clone()
                                    } else {
                                        format!("{}.{}", rpath.0, rpath.1.join("."))
                                    };
                                    self.diagnostics.push(
                                        Diagnostic::error(
                                            "E0302",
                                            format!(
                                                "conflicting access capabilities on `{}` in method call",
                                                path_str
                                            ),
                                            arg.expr.span.clone(),
                                        )
                                        .with_label("cannot borrow receiver while mutating method is invoked")
                                        .with_help("separate the operation into sequential statements"),
                                    );
                                }
                            }
                        }
                    }
                }

                self.check_call_arguments(args, None);
            }
            ExprKind::Index { base, index } => {
                self.check_expr(base, required_cap);
                self.check_expr(index, Capability::Read);
            }
            ExprKind::StructInit { fields, .. } => {
                for (_, fexpr) in fields {
                    self.check_expr(fexpr, Capability::Read);
                }
            }
            ExprKind::ArrayLit(elems) => {
                for e in elems {
                    self.check_expr(e, Capability::Read);
                }
            }
            ExprKind::Range { start, end, .. } => {
                self.check_expr(start, Capability::Read);
                self.check_expr(end, Capability::Read);
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.check_expr(cond, Capability::Read);
                self.analyze_block(then_block);
                if let Some(eb) = else_branch {
                    self.check_expr(eb, Capability::Read);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.check_expr(scrutinee, Capability::Read);
                for arm in arms {
                    self.analyze_block(&arm.body);
                }
            }
            ExprKind::Closure { body, .. } | ExprKind::Block(body) => {
                self.analyze_block(body);
            }
            ExprKind::Cast { expr: inner, .. } | ExprKind::TryProp(inner) => {
                self.check_expr(inner, Capability::Read);
            }
            _ => {}
        }
    }

    fn check_call_arguments(&mut self, args: &[CallArg], fn_sym: Option<&FnSymbol>) {
        // Track active capability paths across arguments of the same call to enforce Law of Exclusivity
        // while allowing Field-Disjoint Borrowing!
        let mut active_borrows: Vec<((String, Vec<String>), Capability, Span)> = Vec::new();

        for (idx, arg) in args.iter().enumerate() {
            let param_cap = fn_sym
                .and_then(|s| s.params.get(idx))
                .map(|(_, c, _)| *c)
                .unwrap_or(Capability::Value);

            let effective_cap = if arg.capability != Capability::Value {
                arg.capability
            } else if let ExprKind::Unary { op, .. } = &arg.expr.kind {
                match op {
                    UnaryOp::Read => Capability::Read,
                    UnaryOp::Write => Capability::Write,
                    UnaryOp::Take => Capability::Take,
                    _ => param_cap,
                }
            } else {
                param_cap
            };

            if let Some(path) = Self::extract_access_path(&arg.expr) {
                for (prev_path, prev_cap, prev_span) in &active_borrows {
                    if Self::paths_overlap(&path, prev_path)
                        && (effective_cap == Capability::Write
                            || effective_cap == Capability::Take
                            || *prev_cap == Capability::Write
                            || *prev_cap == Capability::Take)
                    {
                        let path_str = if path.1.is_empty() {
                            path.0.clone()
                        } else {
                            format!("{}.{}", path.0, path.1.join("."))
                        };
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0302",
                                format!(
                                    "conflicting access capabilities on `{}` in function call",
                                    path_str
                                ),
                                arg.expr.span.clone(),
                            )
                            .with_label(format!(
                                "cannot acquire `{}` access while `{}` access is already active",
                                effective_cap.as_str(),
                                prev_cap.as_str()
                            ))
                            .with_note(format!(
                                "conflicting borrow occurs at line {}, column {}",
                                prev_span.line, prev_span.col
                            ))
                            .with_help(
                                "separate the mutations into sequential calls or borrow disjoint struct fields",
                            ),
                        );
                    }
                }
                if effective_cap != Capability::Value {
                    active_borrows.push((path, effective_cap, arg.expr.span.clone()));
                }
            }

            match effective_cap {
                Capability::Take => self.consume_value(&arg.expr, &arg.expr.span),
                Capability::Write => self.check_expr(&arg.expr, Capability::Write),
                Capability::Read | Capability::Value => {
                    self.check_expr(&arg.expr, Capability::Read)
                }
            }
        }
        let _ = HashSet::<String>::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;
    use crate::compiler::semantic::SemanticAnalyzer;

    fn run_ama(src: &str) -> (DiagnosticBag, AmaReport) {
        let mut lexer = Lexer::new("src/main.aps", src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program();
        let mut sem = SemanticAnalyzer::new();
        sem.analyze_program(&prog);
        let mut ama = AmaAnalyzer::new(sem.tcx, sem.functions);
        ama.analyze_program(&prog);
        (ama.diagnostics, ama.report)
    }

    #[test]
    fn test_ama_rejects_read_only_mutation_e0042() {
        let src = r#"
struct Driver {
    id: i64
    name: String
    balance: i64
}

fn bad_modify(driver: read Driver) {
    driver.balance += 100
}
"#;
        let (diags, _) = run_ama(src);
        assert!(diags.has_errors());
        assert_eq!(diags.diagnostics[0].code, "E0042");
        assert_eq!(
            diags.diagnostics[0].message,
            "cannot modify a read-only value"
        );
    }

    #[test]
    fn test_ama_rejects_use_after_take_e0201() {
        let src = r#"
struct Data {
    payload: String
}

fn consume(d: take Data) {
    println(d.payload)
}

fn main() {
    let item = Data { payload: "secret" }
    consume(take item)
    println(item.payload)
}
"#;
        let (diags, _) = run_ama(src);
        assert!(diags.has_errors());
        assert_eq!(diags.diagnostics[0].code, "E0201");
    }

    #[test]
    fn test_ama_allows_field_disjoint_borrowing_and_rejects_same_field_e0302() {
        let valid_src = r#"
struct Account {
    checking: i64
    savings: i64
}

fn transfer_step(a: write i64, b: read i64) {}

fn main() {
    var acc = Account { checking: 100, savings: 200 }
    transfer_step(write acc.checking, read acc.savings)
}
"#;
        let (diags_ok, _) = run_ama(valid_src);
        assert!(!diags_ok.has_errors());

        let invalid_src = r#"
struct Account {
    checking: i64
    savings: i64
}

fn conflict_step(a: write i64, b: read i64) {}

fn main() {
    var acc = Account { checking: 100, savings: 200 }
    conflict_step(write acc.checking, read acc.checking)
}
"#;
        let (diags_err, _) = run_ama(invalid_src);
        assert!(diags_err.has_errors());
        assert_eq!(diags_err.diagnostics[0].code, "E0302");
    }

    #[test]
    fn test_ama_rejects_region_escape_e0303() {
        let src = r#"
struct Node {
    val: i64
}

fn main() {
    var escaped = Node { val: 0 }
    region arena {
        let local_node = arena.alloc(Node { val: 42 })
        escaped = local_node
    }
}
"#;
        let (diags, _) = run_ama(src);
        assert!(diags.has_errors());
        assert_eq!(diags.diagnostics[0].code, "E0303");
    }
}
