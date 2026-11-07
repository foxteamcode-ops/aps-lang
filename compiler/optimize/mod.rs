use crate::compiler::ast::*;
use crate::compiler::ir::{ApsIrFunction, ApsIrModule};
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct OptStats {
    pub constants_folded: usize,
    pub dead_stmts_removed: usize,
    pub algebraic_simplifications: usize,
    pub functions_inlined: usize,
}

pub struct Optimizer {
    pub opt_level: u8,
    pub stats: OptStats,
    pub known_consts: HashMap<String, Expr>,
    pub inline_candidates: HashMap<String, (Vec<String>, Expr)>,
}

impl Optimizer {
    pub fn new(opt_level: u8) -> Self {
        Self {
            opt_level,
            stats: OptStats::default(),
            known_consts: HashMap::new(),
            inline_candidates: HashMap::new(),
        }
    }

    pub fn optimize_module(&mut self, module: &mut ApsIrModule) {
        if self.opt_level == 0 {
            return;
        }

        // Register module-level constants
        for (name, _, expr) in &module.consts {
            let folded = self.optimize_expr(expr);
            self.known_consts.insert(name.clone(), folded);
        }

        // Collect small pure functions as inline candidates
        for f in &module.functions {
            if f.name != "main" && !f.is_async && f.body.stmts.len() == 1 {
                let param_names: Vec<String> =
                    f.params.iter().map(|(n, _, _, _)| n.clone()).collect();
                match &f.body.stmts[0] {
                    Stmt::Return {
                        value: Some(ret_expr),
                        ..
                    } => {
                        if !Self::expr_contains_call_to(ret_expr, &f.name) {
                            self.inline_candidates
                                .insert(f.name.clone(), (param_names, ret_expr.clone()));
                        }
                    }
                    Stmt::Expr {
                        expr: ret_expr,
                        has_semi: false,
                        ..
                    } => {
                        if !Self::expr_contains_call_to(ret_expr, &f.name) {
                            self.inline_candidates
                                .insert(f.name.clone(), (param_names, ret_expr.clone()));
                        }
                    }
                    _ => {}
                }
            }
        }

        for f in &mut module.functions {
            self.optimize_function(f);
        }
    }

    fn expr_contains_call_to(expr: &Expr, target: &str) -> bool {
        match &expr.kind {
            ExprKind::Call { callee, args } => {
                if let ExprKind::Ident(n) = &callee.kind {
                    if n == target {
                        return true;
                    }
                }
                args.iter()
                    .any(|a| Self::expr_contains_call_to(&a.expr, target))
            }
            ExprKind::Binary { left, right, .. } => {
                Self::expr_contains_call_to(left, target)
                    || Self::expr_contains_call_to(right, target)
            }
            ExprKind::Unary { expr: inner, .. } => Self::expr_contains_call_to(inner, target),
            _ => false,
        }
    }

    fn optimize_function(&mut self, func: &mut ApsIrFunction) {
        let mut local_consts = self.known_consts.clone();
        func.body = self.optimize_block(&func.body, &mut local_consts);
    }

    fn optimize_block(
        &mut self,
        block: &Block,
        local_consts: &mut HashMap<String, Expr>,
    ) -> Block {
        let mut new_stmts = Vec::new();
        let mut terminated = false;

        for stmt in &block.stmts {
            if terminated {
                // Dead Code Elimination: statements after return/break/continue are unreachable
                self.stats.dead_stmts_removed += 1;
                continue;
            }

            match stmt {
                Stmt::Let {
                    name,
                    ty,
                    init,
                    span,
                } => {
                    let opt_init = self.optimize_expr_with_env(init, local_consts);
                    if matches!(
                        opt_init.kind,
                        ExprKind::IntLit(_, _)
                            | ExprKind::FloatLit(_, _)
                            | ExprKind::BoolLit(_)
                            | ExprKind::StringLit(_)
                    ) {
                        local_consts.insert(name.clone(), opt_init.clone());
                    }
                    new_stmts.push(Stmt::Let {
                        name: name.clone(),
                        ty: ty.clone(),
                        init: opt_init,
                        span: span.clone(),
                    });
                }
                Stmt::Var {
                    name,
                    ty,
                    init,
                    span,
                } => {
                    local_consts.remove(name);
                    let opt_init = self.optimize_expr_with_env(init, local_consts);
                    new_stmts.push(Stmt::Var {
                        name: name.clone(),
                        ty: ty.clone(),
                        init: opt_init,
                        span: span.clone(),
                    });
                }
                Stmt::Const(c) => {
                    let opt_val = self.optimize_expr_with_env(&c.value, local_consts);
                    local_consts.insert(c.name.clone(), opt_val.clone());
                    new_stmts.push(Stmt::Const(ConstDecl {
                        is_pub: c.is_pub,
                        name: c.name.clone(),
                        ty: c.ty.clone(),
                        value: opt_val,
                        span: c.span.clone(),
                    }));
                }
                Stmt::Assign {
                    target,
                    op,
                    value,
                    span,
                } => {
                    if let ExprKind::Ident(n) = &target.kind {
                        local_consts.remove(n);
                    }
                    let opt_val = self.optimize_expr_with_env(value, local_consts);
                    new_stmts.push(Stmt::Assign {
                        target: target.clone(),
                        op: *op,
                        value: opt_val,
                        span: span.clone(),
                    });
                }
                Stmt::Return { value, span } => {
                    let opt_val = value
                        .as_ref()
                        .map(|v| self.optimize_expr_with_env(v, local_consts));
                    new_stmts.push(Stmt::Return {
                        value: opt_val,
                        span: span.clone(),
                    });
                    terminated = true;
                }
                Stmt::Break { span } => {
                    new_stmts.push(Stmt::Break { span: span.clone() });
                    terminated = true;
                }
                Stmt::Continue { span } => {
                    new_stmts.push(Stmt::Continue { span: span.clone() });
                    terminated = true;
                }
                Stmt::While { cond, body, span } => {
                    // Don't propagate constants mutated inside the loop!
                    let mut loop_consts = HashMap::new();
                    let opt_cond = self.optimize_expr_with_env(cond, &mut loop_consts);
                    if let ExprKind::BoolLit(false) = opt_cond.kind {
                        self.stats.dead_stmts_removed += 1;
                        continue;
                    }
                    let opt_body = self.optimize_block(body, &mut loop_consts);
                    local_consts.clear();
                    new_stmts.push(Stmt::While {
                        cond: opt_cond,
                        body: opt_body,
                        span: span.clone(),
                    });
                }
                Stmt::Loop { body, span } => {
                    let mut loop_consts = HashMap::new();
                    let opt_body = self.optimize_block(body, &mut loop_consts);
                    local_consts.clear();
                    new_stmts.push(Stmt::Loop {
                        body: opt_body,
                        span: span.clone(),
                    });
                }
                Stmt::Region { name, body, span } => {
                    let opt_body = self.optimize_block(body, local_consts);
                    new_stmts.push(Stmt::Region {
                        name: name.clone(),
                        body: opt_body,
                        span: span.clone(),
                    });
                }
                Stmt::Defer { body, span } => {
                    let opt_body = self.optimize_block(body, local_consts);
                    new_stmts.push(Stmt::Defer {
                        body: opt_body,
                        span: span.clone(),
                    });
                }
                Stmt::Unsafe { body, span } => {
                    let opt_body = self.optimize_block(body, local_consts);
                    new_stmts.push(Stmt::Unsafe {
                        body: opt_body,
                        span: span.clone(),
                    });
                }
                Stmt::For {
                    var,
                    iter,
                    body,
                    span,
                } => {
                    let mut loop_consts = HashMap::new();
                    let opt_iter = self.optimize_expr_with_env(iter, &mut loop_consts);
                    let opt_body = self.optimize_block(body, &mut loop_consts);
                    local_consts.clear();
                    new_stmts.push(Stmt::For {
                        var: var.clone(),
                        iter: opt_iter,
                        body: opt_body,
                        span: span.clone(),
                    });
                }
                Stmt::Expr {
                    expr,
                    has_semi,
                    span,
                } => {
                    let opt_expr = self.optimize_expr_with_env(expr, local_consts);
                    new_stmts.push(Stmt::Expr {
                        expr: opt_expr,
                        has_semi: *has_semi,
                        span: span.clone(),
                    });
                }
            }
        }

        Block {
            stmts: new_stmts,
            span: block.span.clone(),
        }
    }

    pub fn optimize_expr(&mut self, expr: &Expr) -> Expr {
        let mut env = self.known_consts.clone();
        self.optimize_expr_with_env(expr, &mut env)
    }

    fn optimize_expr_with_env(
        &mut self,
        expr: &Expr,
        env: &mut HashMap<String, Expr>,
    ) -> Expr {
        let span = expr.span.clone();
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some(const_expr) = env.get(name) {
                    self.stats.constants_folded += 1;
                    return const_expr.clone();
                }
                expr.clone()
            }
            ExprKind::Unary { op, expr: inner } => {
                let opt_inner = self.optimize_expr_with_env(inner, env);
                match (op, &opt_inner.kind) {
                    (UnaryOp::Neg, ExprKind::IntLit(v, suf)) => {
                        self.stats.constants_folded += 1;
                        Expr {
                            kind: ExprKind::IntLit(-v, suf.clone()),
                            span,
                        }
                    }
                    (UnaryOp::Neg, ExprKind::FloatLit(v, suf)) => {
                        self.stats.constants_folded += 1;
                        Expr {
                            kind: ExprKind::FloatLit(-v, suf.clone()),
                            span,
                        }
                    }
                    (UnaryOp::Not, ExprKind::BoolLit(b)) => {
                        self.stats.constants_folded += 1;
                        Expr {
                            kind: ExprKind::BoolLit(!b),
                            span,
                        }
                    }
                    _ => Expr {
                        kind: ExprKind::Unary {
                            op: *op,
                            expr: Box::new(opt_inner),
                        },
                        span,
                    },
                }
            }
            ExprKind::Binary { op, left, right } => {
                let l = self.optimize_expr_with_env(left, env);
                let r = self.optimize_expr_with_env(right, env);

                // Constant Folding for integers
                if let (ExprKind::IntLit(lv, lsuf), ExprKind::IntLit(rv, _)) = (&l.kind, &r.kind) {
                    let folded = match op {
                        BinOp::Add => Some(ExprKind::IntLit(lv.wrapping_add(*rv), lsuf.clone())),
                        BinOp::Sub => Some(ExprKind::IntLit(lv.wrapping_sub(*rv), lsuf.clone())),
                        BinOp::Mul => Some(ExprKind::IntLit(lv.wrapping_mul(*rv), lsuf.clone())),
                        BinOp::Div if *rv != 0 => {
                            Some(ExprKind::IntLit(lv.wrapping_div(*rv), lsuf.clone()))
                        }
                        BinOp::Rem if *rv != 0 => {
                            Some(ExprKind::IntLit(lv.wrapping_rem(*rv), lsuf.clone()))
                        }
                        BinOp::BitAnd => Some(ExprKind::IntLit(lv & rv, lsuf.clone())),
                        BinOp::BitOr => Some(ExprKind::IntLit(lv | rv, lsuf.clone())),
                        BinOp::BitXor => Some(ExprKind::IntLit(lv ^ rv, lsuf.clone())),
                        BinOp::Shl => Some(ExprKind::IntLit(lv.wrapping_shl(*rv as u32), lsuf.clone())),
                        BinOp::Shr => Some(ExprKind::IntLit(lv.wrapping_shr(*rv as u32), lsuf.clone())),
                        BinOp::Eq => Some(ExprKind::BoolLit(lv == rv)),
                        BinOp::Ne => Some(ExprKind::BoolLit(lv != rv)),
                        BinOp::Lt => Some(ExprKind::BoolLit(lv < rv)),
                        BinOp::Le => Some(ExprKind::BoolLit(lv <= rv)),
                        BinOp::Gt => Some(ExprKind::BoolLit(lv > rv)),
                        BinOp::Ge => Some(ExprKind::BoolLit(lv >= rv)),
                        _ => None,
                    };
                    if let Some(kind) = folded {
                        self.stats.constants_folded += 1;
                        return Expr { kind, span };
                    }
                }

                // Constant Folding for strings
                if let (ExprKind::StringLit(ls), ExprKind::StringLit(rs)) = (&l.kind, &r.kind) {
                    let folded = match op {
                        BinOp::Eq => Some(ExprKind::BoolLit(ls == rs)),
                        BinOp::Ne => Some(ExprKind::BoolLit(ls != rs)),
                        BinOp::Add => Some(ExprKind::StringLit(format!("{}{}", ls, rs))),
                        _ => None,
                    };
                    if let Some(kind) = folded {
                        self.stats.constants_folded += 1;
                        return Expr { kind, span };
                    }
                }


                // Constant Folding for floats
                if let (ExprKind::FloatLit(lv, lsuf), ExprKind::FloatLit(rv, _)) = (&l.kind, &r.kind)
                {
                    let folded = match op {
                        BinOp::Add => Some(ExprKind::FloatLit(lv + rv, lsuf.clone())),
                        BinOp::Sub => Some(ExprKind::FloatLit(lv - rv, lsuf.clone())),
                        BinOp::Mul => Some(ExprKind::FloatLit(lv * rv, lsuf.clone())),
                        BinOp::Div => Some(ExprKind::FloatLit(lv / rv, lsuf.clone())),
                        _ => None,
                    };
                    if let Some(kind) = folded {
                        self.stats.constants_folded += 1;
                        return Expr { kind, span };
                    }
                }

                // Algebraic Simplifications
                match (op, &l.kind, &r.kind) {
                    (BinOp::Add | BinOp::Sub, _, ExprKind::IntLit(0, _)) => {
                        self.stats.algebraic_simplifications += 1;
                        return l;
                    }
                    (BinOp::Add, ExprKind::IntLit(0, _), _) => {
                        self.stats.algebraic_simplifications += 1;
                        return r;
                    }
                    (BinOp::Mul | BinOp::Div, _, ExprKind::IntLit(1, _)) => {
                        self.stats.algebraic_simplifications += 1;
                        return l;
                    }
                    (BinOp::Mul, ExprKind::IntLit(1, _), _) => {
                        self.stats.algebraic_simplifications += 1;
                        return r;
                    }
                    _ => {}
                }

                Expr {
                    kind: ExprKind::Binary {
                        op: *op,
                        left: Box::new(l),
                        right: Box::new(r),
                    },
                    span,
                }
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                let opt_cond = self.optimize_expr_with_env(cond, env);
                let mut then_env = env.clone();
                let opt_then = self.optimize_block(then_block, &mut then_env);
                let opt_else = else_branch
                    .as_ref()
                    .map(|eb| Box::new(self.optimize_expr_with_env(eb, &mut env.clone())));

                if let ExprKind::BoolLit(true) = opt_cond.kind {
                    self.stats.dead_stmts_removed += 1;
                    return Expr {
                        kind: ExprKind::Block(opt_then),
                        span,
                    };
                }
                if let ExprKind::BoolLit(false) = opt_cond.kind {
                    self.stats.dead_stmts_removed += 1;
                    if let Some(eb) = opt_else {
                        return *eb;
                    }
                }

                Expr {
                    kind: ExprKind::If {
                        cond: Box::new(opt_cond),
                        then_block: opt_then,
                        else_branch: opt_else,
                    },
                    span,
                }
            }
            ExprKind::Call { callee, args } => {
                let opt_args: Vec<CallArg> = args
                    .iter()
                    .map(|a| CallArg {
                        capability: a.capability,
                        expr: self.optimize_expr_with_env(&a.expr, env),
                    })
                    .collect();

                // Function Inlining if all arguments are simple literals/idents and function is in inline_candidates
                if self.opt_level >= 2 {
                    if let ExprKind::Ident(fn_name) = &callee.kind {
                        if let Some((param_names, body_expr)) =
                            self.inline_candidates.get(fn_name).cloned()
                        {
                            if param_names.len() == opt_args.len()
                                && opt_args.iter().all(|a| {
                                    matches!(
                                        a.expr.kind,
                                        ExprKind::IntLit(_, _)
                                            | ExprKind::FloatLit(_, _)
                                            | ExprKind::BoolLit(_)
                                            | ExprKind::Ident(_)
                                    )
                                })
                            {
                                let mut inline_map = HashMap::new();
                                for (pname, arg) in param_names.iter().zip(opt_args.iter()) {
                                    inline_map.insert(pname.clone(), arg.expr.clone());
                                }
                                let substituted = Self::substitute_params(&body_expr, &inline_map);
                                self.stats.functions_inlined += 1;
                                return self.optimize_expr_with_env(&substituted, env);
                            }
                        }
                    }
                }

                Expr {
                    kind: ExprKind::Call {
                        callee: callee.clone(),
                        args: opt_args,
                    },
                    span,
                }
            }
            ExprKind::MethodCall { receiver, method, args } => {
                let opt_recv = self.optimize_expr_with_env(receiver, env);
                if (method == "len" || method == "byte_len") && args.is_empty() {
                    if let ExprKind::StringLit(s) = &opt_recv.kind {
                        self.stats.constants_folded += 1;
                        return Expr {
                            kind: ExprKind::IntLit(s.len() as i64, None),
                            span,
                        };
                    }
                }
                let opt_args = args
                    .iter()
                    .map(|a| CallArg {
                        capability: a.capability,
                        expr: self.optimize_expr_with_env(&a.expr, env),
                    })
                    .collect();
                Expr {
                    kind: ExprKind::MethodCall {
                        receiver: Box::new(opt_recv),
                        method: method.clone(),
                        args: opt_args,
                    },
                    span,
                }
            }
            _ => expr.clone(),
        }
    }

    fn substitute_block(block: &Block, map: &HashMap<String, Expr>) -> Block {
        let stmts = block
            .stmts
            .iter()
            .map(|s| match s {
                Stmt::Expr {
                    expr,
                    has_semi,
                    span,
                } => Stmt::Expr {
                    expr: Self::substitute_params(expr, map),
                    has_semi: *has_semi,
                    span: span.clone(),
                },
                Stmt::Return { value, span } => Stmt::Return {
                    value: value.as_ref().map(|v| Self::substitute_params(v, map)),
                    span: span.clone(),
                },
                other => other.clone(),
            })
            .collect();
        Block {
            stmts,
            span: block.span.clone(),
        }
    }

    fn substitute_params(expr: &Expr, map: &HashMap<String, Expr>) -> Expr {
        let span = expr.span.clone();
        match &expr.kind {
            ExprKind::Ident(n) => {
                if let Some(rep) = map.get(n) {
                    rep.clone()
                } else {
                    expr.clone()
                }
            }
            ExprKind::Binary { op, left, right } => Expr {
                kind: ExprKind::Binary {
                    op: *op,
                    left: Box::new(Self::substitute_params(left, map)),
                    right: Box::new(Self::substitute_params(right, map)),
                },
                span,
            },
            ExprKind::Unary { op, expr: inner } => Expr {
                kind: ExprKind::Unary {
                    op: *op,
                    expr: Box::new(Self::substitute_params(inner, map)),
                },
                span,
            },
            ExprKind::FieldAccess { base, field } => Expr {
                kind: ExprKind::FieldAccess {
                    base: Box::new(Self::substitute_params(base, map)),
                    field: field.clone(),
                },
                span,
            },
            ExprKind::Call { callee, args } => Expr {
                kind: ExprKind::Call {
                    callee: callee.clone(),
                    args: args
                        .iter()
                        .map(|a| CallArg {
                            capability: a.capability,
                            expr: Self::substitute_params(&a.expr, map),
                        })
                        .collect(),
                },
                span,
            },
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => Expr {
                kind: ExprKind::If {
                    cond: Box::new(Self::substitute_params(cond, map)),
                    then_block: Self::substitute_block(then_block, map),
                    else_branch: else_branch
                        .as_ref()
                        .map(|eb| Box::new(Self::substitute_params(eb, map))),
                },
                span,
            },
            ExprKind::Block(blk) => Expr {
                kind: ExprKind::Block(Self::substitute_block(blk, map)),
                span,
            },
            ExprKind::Cast { expr: inner, ty } => Expr {
                kind: ExprKind::Cast {
                    expr: Box::new(Self::substitute_params(inner, map)),
                    ty: ty.clone(),
                },
                span,
            },
            ExprKind::MethodCall { receiver, method, args } => Expr {
                kind: ExprKind::MethodCall {
                    receiver: Box::new(Self::substitute_params(receiver, map)),
                    method: method.clone(),
                    args: args
                        .iter()
                        .map(|a| CallArg {
                            capability: a.capability,
                            expr: Self::substitute_params(&a.expr, map),
                        })
                        .collect(),
                },
                span,
            },
            _ => expr.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ama::AmaReport;
    use crate::compiler::hir::HirLowering;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::mir::MirBuilder;
    use crate::compiler::parser::Parser;
    use crate::compiler::semantic::SemanticAnalyzer;

    #[test]
    fn test_constant_folding_and_inlining() {
        let src = r#"
fn square(x: i64) -> i64 {
    return x * x
}

fn main() {
    let a = 2 + 3 * 4
    let b = square(5)
    println(a + b)
}
"#;
        let mut lexer = Lexer::new("test.aps", src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let prog = parser.parse_program();
        let mut sem = SemanticAnalyzer::new();
        sem.analyze_program(&prog);
        let hir = HirLowering::new(sem.tcx).lower_program(&prog);
        let mir = MirBuilder::build(hir, AmaReport::default());
        let mut ir = ApsIrModule::from_mir(mir);
        let mut opt = Optimizer::new(3);
        opt.optimize_module(&mut ir);
        assert!(opt.stats.constants_folded >= 2);
        assert!(opt.stats.functions_inlined >= 1);
    }
}
