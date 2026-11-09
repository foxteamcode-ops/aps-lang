//! AST-level Monomorphization Pass for APS Generics
//! Specializes generic functions and structs per concrete type arguments.

use crate::compiler::ast::*;
use std::collections::{HashMap, HashSet};

pub fn monomorphize_program(program: &mut Program) {
    loop {
        let specialized_names = collect_specialized_names(program);
        let mut added_any = false;

        let template_fns: HashMap<String, FnDecl> = program
            .items
            .iter()
            .filter_map(|it| {
                if let Item::Function(f) = it {
                    if !f.generics.is_empty() {
                        Some((f.name.clone(), f.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        let template_structs: HashMap<String, StructDecl> = program
            .items
            .iter()
            .filter_map(|it| {
                if let Item::Struct(s) = it {
                    if !s.generics.is_empty() {
                        Some((s.name.clone(), s.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        for spec_name in &specialized_names {
            let already_exists = program.items.iter().any(|it| match it {
                Item::Function(f) => f.name == *spec_name,
                Item::Struct(s) => s.name == *spec_name,
                _ => false,
            });
            if already_exists {
                continue;
            }

            let parts: Vec<&str> = spec_name.split('$').collect();
            if parts.len() < 2 {
                continue;
            }
            let base = parts[0];
            let type_args = &parts[1..];

            if let Some(tmpl) = template_fns.get(base) {
                if tmpl.generics.len() == type_args.len() {
                    let mut map = HashMap::new();
                    for (g, a) in tmpl.generics.iter().zip(type_args.iter()) {
                        map.insert(g.name.clone(), a.to_string());
                    }
                    let mut spec_fn = tmpl.clone();
                    spec_fn.name = spec_name.clone();
                    spec_fn.generics.clear();
                    for p in &mut spec_fn.params {
                        p.ty = subst_type_ast(&p.ty, &map);
                    }
                    spec_fn.ret_type = subst_type_ast(&spec_fn.ret_type, &map);
                    subst_block(&mut spec_fn.body, &map);
                    program.items.push(Item::Function(spec_fn));
                    added_any = true;
                }
            }

            if let Some(tmpl) = template_structs.get(base) {
                if tmpl.generics.len() == type_args.len() {
                    let mut map = HashMap::new();
                    for (g, a) in tmpl.generics.iter().zip(type_args.iter()) {
                        map.insert(g.name.clone(), a.to_string());
                    }
                    let mut spec_st = tmpl.clone();
                    spec_st.name = spec_name.clone();
                    spec_st.generics.clear();
                    for f in &mut spec_st.fields {
                        f.ty = subst_type_ast(&f.ty, &map);
                    }
                    program.items.push(Item::Struct(spec_st));
                    added_any = true;
                }
            }
        }

        if !added_any {
            break;
        }
    }
}

fn subst_type_ast(ty: &TypeAst, map: &HashMap<String, String>) -> TypeAst {
    match ty {
        TypeAst::Named { name, args } => {
            let new_name = if let Some(concrete) = map.get(name) {
                concrete.clone()
            } else if name.contains('$') {
                let parts: Vec<&str> = name.split('$').collect();
                let replaced_parts: Vec<String> = parts
                    .iter()
                    .map(|p| map.get(*p).cloned().unwrap_or_else(|| p.to_string()))
                    .collect();
                replaced_parts.join("$")
            } else {
                name.clone()
            };
            let new_args: Vec<TypeAst> = args.iter().map(|a| subst_type_ast(a, map)).collect();
            let is_builtin = matches!(
                new_name.as_str(),
                "Vec"
                    | "HashMap"
                    | "HashSet"
                    | "Option"
                    | "Result"
                    | "Channel"
                    | "channel"
                    | "Receiver"
                    | "Sender"
                    | "Task"
                    | "Shared"
                    | "Weak"
                    | "Mutex"
                    | "spawn_thread"
                    | "block_on"
                    | "promote"
            );
            if !is_builtin && !new_args.is_empty() {
                let mut arg_names = Vec::new();
                for a in &new_args {
                    match a {
                        TypeAst::Named { name: aname, .. } => arg_names.push(aname.clone()),
                        _ => {}
                    }
                }
                if arg_names.len() == new_args.len() {
                    return TypeAst::Named {
                        name: format!("{}${}", new_name, arg_names.join("$")),
                        args: Vec::new(),
                    };
                }
            }
            TypeAst::Named {
                name: new_name,
                args: new_args,
            }
        }
        TypeAst::Capability { cap, inner } => TypeAst::Capability {
            cap: *cap,
            inner: Box::new(subst_type_ast(inner, map)),
        },
        TypeAst::Array { elem, size } => TypeAst::Array {
            elem: Box::new(subst_type_ast(elem, map)),
            size: *size,
        },
        TypeAst::Function { params, ret } => TypeAst::Function {
            params: params.iter().map(|p| subst_type_ast(p, map)).collect(),
            ret: Box::new(subst_type_ast(ret, map)),
        },
        TypeAst::Shared(inner) => TypeAst::Shared(Box::new(subst_type_ast(inner, map))),
        TypeAst::Pointer(inner) => TypeAst::Pointer(Box::new(subst_type_ast(inner, map))),
        other => other.clone(),
    }
}

fn subst_block(block: &mut Block, map: &HashMap<String, String>) {
    for stmt in &mut block.stmts {
        subst_stmt(stmt, map);
    }
}

fn subst_stmt(stmt: &mut Stmt, map: &HashMap<String, String>) {
    match stmt {
        Stmt::Let { ty, init, .. } | Stmt::Var { ty, init, .. } => {
            *ty = subst_type_ast(ty, map);
            subst_expr(init, map);
        }
        Stmt::Assign { target, value, .. } => {
            subst_expr(target, map);
            subst_expr(value, map);
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                subst_expr(v, map);
            }
        }
        Stmt::Expr { expr, .. } => subst_expr(expr, map),
        Stmt::While { cond, body, .. } => {
            subst_expr(cond, map);
            subst_block(body, map);
        }
        Stmt::For { iter, body, .. } => {
            subst_expr(iter, map);
            subst_block(body, map);
        }
        Stmt::Loop { body, .. }
        | Stmt::Defer { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Unsafe { body, .. } => subst_block(body, map),
        _ => {}
    }
}

fn subst_expr(expr: &mut Expr, map: &HashMap<String, String>) {
    match &mut expr.kind {
        ExprKind::Cast { expr: inner, ty } => {
            subst_expr(inner, map);
            *ty = subst_type_ast(ty, map);
        }
        ExprKind::Binary { left, right, .. } => {
            subst_expr(left, map);
            subst_expr(right, map);
        }
        ExprKind::Unary { expr: inner, .. } => subst_expr(inner, map),
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(name) = &mut callee.kind {
                if let Some(concrete) = map.get(name) {
                    *name = concrete.clone();
                } else if name.contains('$') {
                    let parts: Vec<&str> = name.split('$').collect();
                    let replaced: Vec<String> = parts
                        .iter()
                        .map(|p| map.get(*p).cloned().unwrap_or_else(|| p.to_string()))
                        .collect();
                    *name = replaced.join("$");
                }
            }
            subst_expr(callee, map);
            for a in args {
                subst_expr(&mut a.expr, map);
            }
        }
        ExprKind::MethodCall { receiver, method, args } => {
            if method.contains('$') {
                let parts: Vec<&str> = method.split('$').collect();
                let replaced: Vec<String> = parts
                    .iter()
                    .map(|p| map.get(*p).cloned().unwrap_or_else(|| p.to_string()))
                    .collect();
                *method = replaced.join("$");
            }
            subst_expr(receiver, map);
            for a in args {
                subst_expr(&mut a.expr, map);
            }
        }
        ExprKind::FieldAccess { base, .. } => subst_expr(base, map),
        ExprKind::Index { base, index } => {
            subst_expr(base, map);
            subst_expr(index, map);
        }
        ExprKind::StructInit { name, fields } => {
            if let Some(concrete) = map.get(name) {
                *name = concrete.clone();
            } else if name.contains('$') {
                let parts: Vec<&str> = name.split('$').collect();
                let replaced: Vec<String> = parts
                    .iter()
                    .map(|p| map.get(*p).cloned().unwrap_or_else(|| p.to_string()))
                    .collect();
                *name = replaced.join("$");
            }
            for (_, fe) in fields {
                subst_expr(fe, map);
            }
        }
        ExprKind::ArrayLit(items) => {
            for it in items {
                subst_expr(it, map);
            }
        }
        ExprKind::If { cond, then_block, else_branch } => {
            subst_expr(cond, map);
            subst_block(then_block, map);
            if let Some(eb) = else_branch {
                subst_expr(eb, map);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            subst_expr(scrutinee, map);
            for arm in arms {
                subst_block(&mut arm.body, map);
            }
        }
        ExprKind::Block(b) => subst_block(b, map),
        ExprKind::TryProp(inner) => subst_expr(inner, map),
        _ => {}
    }
}

fn collect_specialized_names(program: &Program) -> HashSet<String> {
    let mut names = HashSet::new();
    for it in &program.items {
        match it {
            Item::Function(f) => {
                for p in &f.params {
                    collect_from_type(&p.ty, &mut names);
                }
                collect_from_type(&f.ret_type, &mut names);
                collect_from_block(&f.body, &mut names);
            }
            Item::Struct(st) => {
                for f in &st.fields {
                    collect_from_type(&f.ty, &mut names);
                }
            }
            Item::Impl(imp) => {
                for m in &imp.methods {
                    for p in &m.params {
                        collect_from_type(&p.ty, &mut names);
                    }
                    collect_from_type(&m.ret_type, &mut names);
                    collect_from_block(&m.body, &mut names);
                }
            }
            _ => {}
        }
    }
    names
}

fn collect_from_type(ty: &TypeAst, names: &mut HashSet<String>) {
    match ty {
        TypeAst::Named { name, args } => {
            if name.contains('$') {
                names.insert(name.clone());
            }
            for a in args {
                collect_from_type(a, names);
            }
        }
        TypeAst::Capability { inner, .. } => collect_from_type(inner, names),
        TypeAst::Array { elem, .. } => collect_from_type(elem, names),
        TypeAst::Function { params, ret } => {
            for p in params {
                collect_from_type(p, names);
            }
            collect_from_type(ret, names);
        }
        TypeAst::Shared(inner) | TypeAst::Pointer(inner) => collect_from_type(inner, names),
        _ => {}
    }
}

fn collect_from_block(block: &Block, names: &mut HashSet<String>) {
    for s in &block.stmts {
        collect_from_stmt(s, names);
    }
}

fn collect_from_stmt(stmt: &Stmt, names: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { ty, init, .. } | Stmt::Var { ty, init, .. } => {
            collect_from_type(ty, names);
            collect_from_expr(init, names);
        }
        Stmt::Assign { target, value, .. } => {
            collect_from_expr(target, names);
            collect_from_expr(value, names);
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                collect_from_expr(v, names);
            }
        }
        Stmt::Expr { expr, .. } => collect_from_expr(expr, names),
        Stmt::While { cond, body, .. } => {
            collect_from_expr(cond, names);
            collect_from_block(body, names);
        }
        Stmt::For { iter, body, .. } => {
            collect_from_expr(iter, names);
            collect_from_block(body, names);
        }
        Stmt::Loop { body, .. }
        | Stmt::Defer { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Unsafe { body, .. } => collect_from_block(body, names),
        _ => {}
    }
}

fn collect_from_expr(expr: &Expr, names: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(name) = &callee.kind {
                if name.contains('$') {
                    names.insert(name.clone());
                }
            }
            collect_from_expr(callee, names);
            for a in args {
                collect_from_expr(&a.expr, names);
            }
        }
        ExprKind::MethodCall { receiver, method, args } => {
            if method.contains('$') {
                names.insert(method.clone());
            }
            collect_from_expr(receiver, names);
            for a in args {
                collect_from_expr(&a.expr, names);
            }
        }
        ExprKind::StructInit { name, fields } => {
            if name.contains('$') {
                names.insert(name.clone());
            }
            for (_, fe) in fields {
                collect_from_expr(fe, names);
            }
        }
        ExprKind::Cast { expr: inner, ty } => {
            collect_from_expr(inner, names);
            collect_from_type(ty, names);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_from_expr(left, names);
            collect_from_expr(right, names);
        }
        ExprKind::Unary { expr: inner, .. } => collect_from_expr(inner, names),
        ExprKind::FieldAccess { base, .. } => collect_from_expr(base, names),
        ExprKind::Index { base, index } => {
            collect_from_expr(base, names);
            collect_from_expr(index, names);
        }
        ExprKind::ArrayLit(items) => {
            for it in items {
                collect_from_expr(it, names);
            }
        }
        ExprKind::If { cond, then_block, else_branch } => {
            collect_from_expr(cond, names);
            collect_from_block(then_block, names);
            if let Some(eb) = else_branch {
                collect_from_expr(eb, names);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_from_expr(scrutinee, names);
            for arm in arms {
                collect_from_block(&arm.body, names);
            }
        }
        ExprKind::Block(b) => collect_from_block(b, names),
        ExprKind::TryProp(inner) => collect_from_expr(inner, names),
        _ => {}
    }
}
