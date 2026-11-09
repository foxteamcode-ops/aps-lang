//! Conservative escape analysis used to decide which heap locals can be freed
//! deterministically at scope exit.
//!
//! A local is *droppable* only when:
//!   * it is initialised directly from `Vec.new()`,
//!   * its name is declared exactly once in the whole function (no shadowing),
//!   * every use is one of: `v.push(..)`, `v.pop()`, `v.len()`, `v[i]` (read or
//!     assignment target), or `for x in v`,
//!   * it is never mentioned inside a closure.
//!
//! Any other mention (returned, passed as argument, aliased, captured, moved,
//! stored in a struct, ...) is treated as an escape and the value is *not*
//! freed. This trades completeness for soundness: it can only leak, never
//! free something that is still reachable.

use crate::compiler::ast::*;
use std::collections::{HashMap, HashSet};

const SAFE_VEC_METHODS: &[&str] = &["push", "pop", "len"];

#[derive(Default)]
struct Scan {
    candidates: HashSet<String>,
    decl_counts: HashMap<String, usize>,
    escaped: HashSet<String>,
}

fn is_vec_new(e: &Expr) -> bool {
    if let ExprKind::MethodCall { receiver, method, args } = &e.kind {
        if method == "new" && args.is_empty() {
            if let ExprKind::Ident(t) = &receiver.kind {
                return t == "Vec";
            }
        }
    }
    false
}

impl Scan {
    fn declare(&mut self, name: &str) {
        *self.decl_counts.entry(name.to_string()).or_insert(0) += 1;
    }

    fn block(&mut self, b: &Block, in_closure: bool) {
        for s in &b.stmts {
            self.stmt(s, in_closure);
        }
    }

    fn stmt(&mut self, s: &Stmt, in_closure: bool) {
        match s {
            Stmt::Let { name, init, .. } | Stmt::Var { name, init, .. } => {
                self.declare(name);
                if is_vec_new(init) && !in_closure {
                    self.candidates.insert(name.clone());
                }
                self.expr(init, in_closure);
            }
            Stmt::Const(c) => {
                self.declare(&c.name);
                self.expr(&c.value, in_closure);
            }
            Stmt::Assign { target, value, .. } => {
                self.expr(target, in_closure);
                self.expr(value, in_closure);
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.expr(v, in_closure);
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Defer { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Loop { body, .. } => self.block(body, in_closure),
            Stmt::While { cond, body, .. } => {
                self.expr(cond, in_closure);
                self.block(body, in_closure);
            }
            Stmt::For { var, iter, body, .. } => {
                self.declare(var);
                // `for x in v` only reads the vector.
                match &iter.kind {
                    ExprKind::Ident(n) if !in_closure => {
                        let _ = n;
                    }
                    _ => self.expr(iter, in_closure),
                }
                self.block(body, in_closure);
            }
            Stmt::Expr { expr, .. } => self.expr(expr, in_closure),
        }
    }

    fn mark(&mut self, name: &str) {
        self.escaped.insert(name.to_string());
    }

    fn expr(&mut self, e: &Expr, in_closure: bool) {
        match &e.kind {
            ExprKind::Ident(n) => self.mark(n),
            ExprKind::IntLit(..)
            | ExprKind::FloatLit(..)
            | ExprKind::StringLit(_)
            | ExprKind::CharLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::Path(_) => {}
            ExprKind::Binary { left, right, .. } => {
                self.expr(left, in_closure);
                self.expr(right, in_closure);
            }
            ExprKind::Unary { expr, .. } => self.expr(expr, in_closure),
            ExprKind::Cast { expr, .. } => self.expr(expr, in_closure),
            ExprKind::Call { callee, args } => {
                self.expr(callee, in_closure);
                for a in args {
                    self.expr(&a.expr, in_closure);
                }
            }
            ExprKind::MethodCall { receiver, method, args } => {
                let safe_recv = matches!(&receiver.kind, ExprKind::Ident(_))
                    && SAFE_VEC_METHODS.contains(&method.as_str())
                    && !in_closure;
                if !safe_recv {
                    self.expr(receiver, in_closure);
                }
                for a in args {
                    self.expr(&a.expr, in_closure);
                }
            }
            ExprKind::FieldAccess { base, .. } => self.expr(base, in_closure),
            ExprKind::Index { base, index } => {
                if !(matches!(&base.kind, ExprKind::Ident(_)) && !in_closure) {
                    self.expr(base, in_closure);
                }
                self.expr(index, in_closure);
            }
            ExprKind::StructInit { fields, .. } => {
                for (_, fe) in fields {
                    self.expr(fe, in_closure);
                }
            }
            ExprKind::ArrayLit(items) => {
                for it in items {
                    self.expr(it, in_closure);
                }
            }
            ExprKind::Range { start, end, .. } => {
                self.expr(start, in_closure);
                self.expr(end, in_closure);
            }
            ExprKind::If { cond, then_block, else_branch } => {
                self.expr(cond, in_closure);
                self.block(then_block, in_closure);
                if let Some(eb) = else_branch {
                    self.expr(eb, in_closure);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee, in_closure);
                for arm in arms {
                    match &arm.pattern {
                        Pattern::Ident(n) => self.declare(n),
                        Pattern::Variant { bindings, .. } => {
                            for b in bindings {
                                self.declare(b);
                            }
                        }
                        Pattern::Literal(l) => self.expr(l, in_closure),
                        Pattern::Wildcard => {}
                    }
                    self.block(&arm.body, in_closure);
                }
            }
            ExprKind::Closure { params, body, .. } => {
                for p in params {
                    self.declare(&p.name);
                }
                self.block(body, true);
            }
            ExprKind::Block(b) => self.block(b, in_closure),
            ExprKind::TryProp(inner) => self.expr(inner, in_closure),
        }
    }
}

/// Returns the names of `Vec.new()` locals in `body` that are safe to free when
/// their declaring scope exits. `param_names` are function parameters (counted
/// as declarations so a local may not shadow them).
pub fn droppable_vec_locals(body: &Block, param_names: &[String]) -> HashSet<String> {
    let mut scan = Scan::default();
    for p in param_names {
        scan.declare(p);
    }
    scan.block(body, false);
    scan.candidates
        .iter()
        .filter(|n| scan.decl_counts.get(*n).copied().unwrap_or(0) == 1 && !scan.escaped.contains(*n))
        .cloned()
        .collect()
}
