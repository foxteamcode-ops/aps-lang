use crate::compiler::ast::*;
use crate::compiler::hir::HirStruct;
use crate::compiler::ir::{ApsIrFunction, ApsIrModule};
use crate::compiler::types::Type;
use crate::runtime::APS_RUNTIME_C_SOURCE;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
struct LocalVar {
    alloca_reg: String,
    ty: Type,
    #[allow(dead_code)]
    is_write_ref: bool, // True if primitive passed by `write` pointer
}

pub struct LlvmBackend {
    pub target_triple: String,
    ir_header: String,
    ir_globals: String,
    ir_functions: String,
    entry_allocas: String,
    current_fn_body: Option<Block>,
    string_pool: Vec<String>,
    string_map: HashMap<String, (usize, usize)>, // content -> (id, byte_len_with_null)
    structs: HashMap<String, Vec<(String, Type)>>,
    enums: HashMap<String, Vec<(String, Vec<Type>)>>,
    fn_ret_types: HashMap<String, Type>,
    fn_param_caps: HashMap<String, Vec<(Capability, Type)>>,
    reg_counter: usize,
    label_counter: usize,
}

impl LlvmBackend {
    pub fn new(target_triple: Option<&str>) -> Self {
        let triple = target_triple
            .map(|s| s.to_string())
            .unwrap_or_else(Self::default_host_triple);
        Self {
            target_triple: triple,
            ir_header: String::new(),
            ir_globals: String::new(),
            ir_functions: String::new(),
            entry_allocas: String::new(),
            current_fn_body: None,
            string_pool: Vec::new(),
            string_map: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            fn_ret_types: HashMap::new(),
            fn_param_caps: HashMap::new(),
            reg_counter: 0,
            label_counter: 0,
        }
    }

    pub fn default_host_triple() -> String {
        if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc".to_string()
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-apple-darwin".to_string()
            } else {
                "x86_64-apple-darwin".to_string()
            }
        } else {
            "x86_64-unknown-linux-gnu".to_string()
        }
    }

    fn next_reg(&mut self) -> String {
        let r = format!("%t{}", self.reg_counter);
        self.reg_counter += 1;
        r
    }

    fn next_label(&mut self, prefix: &str) -> String {
        let l = format!("{}_{}", prefix, self.label_counter);
        self.label_counter += 1;
        l
    }

    fn emit_alloca(&mut self, llvm_ty: &str) -> String {
        let slot = self.next_reg();
        self.entry_allocas
            .push_str(&format!("  {} = alloca {}, align 8\n", slot, llvm_ty));
        slot
    }

    fn is_struct_all_copy(&self, sname: &str) -> bool {
        if let Some(fields) = self.structs.get(sname) {
            if fields.is_empty() {
                return true;
            }
            fields.iter().all(|(_, fty)| match fty.strip_region() {
                Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::Isize
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::Usize
                | Type::F32
                | Type::F64
                | Type::Bool
                | Type::Char => true,
                Type::Struct(nested_name, _) => self.is_struct_all_copy(nested_name),
                _ => false,
            })
        } else {
            false
        }
    }

    fn expr_has_bare_ident(expr: &Expr, var: &str) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => name == var,
            ExprKind::FieldAccess { base, .. } => {
                if let ExprKind::Ident(bname) = &base.kind {
                    if bname == var {
                        return false;
                    }
                }
                Self::expr_has_bare_ident(base, var)
            }
            ExprKind::Unary { expr: inner, .. } => Self::expr_has_bare_ident(inner, var),
            ExprKind::Binary { left, right, .. } => {
                Self::expr_has_bare_ident(left, var) || Self::expr_has_bare_ident(right, var)
            }
            ExprKind::Cast { expr: inner, .. } => Self::expr_has_bare_ident(inner, var),
            ExprKind::Call { callee, args } => {
                Self::expr_has_bare_ident(callee, var)
                    || args.iter().any(|a| Self::expr_has_bare_ident(&a.expr, var))
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                Self::expr_has_bare_ident(receiver, var)
                    || args.iter().any(|a| Self::expr_has_bare_ident(&a.expr, var))
            }
            ExprKind::Index { base, index } => {
                Self::expr_has_bare_ident(base, var) || Self::expr_has_bare_ident(index, var)
            }
            ExprKind::StructInit { fields, .. } => {
                fields.iter().any(|(_, e)| Self::expr_has_bare_ident(e, var))
            }
            ExprKind::ArrayLit(elems) => elems.iter().any(|e| Self::expr_has_bare_ident(e, var)),
            ExprKind::Range { start, end, .. } => {
                Self::expr_has_bare_ident(start, var) || Self::expr_has_bare_ident(end, var)
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                Self::expr_has_bare_ident(cond, var)
                    || Self::block_has_bare_ident(then_block, var)
                    || else_branch
                        .as_ref()
                        .map_or(false, |b| Self::expr_has_bare_ident(b, var))
            }
            ExprKind::Match { scrutinee, arms } => {
                Self::expr_has_bare_ident(scrutinee, var)
                    || arms.iter().any(|arm| Self::block_has_bare_ident(&arm.body, var))
            }
            ExprKind::Block(b) => Self::block_has_bare_ident(b, var),
            ExprKind::TryProp(inner) => Self::expr_has_bare_ident(inner, var),
            _ => false,
        }
    }

    fn block_has_bare_ident(block: &Block, var: &str) -> bool {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let { init, .. } | Stmt::Var { init, .. } => {
                    if Self::expr_has_bare_ident(init, var) {
                        return true;
                    }
                }
                Stmt::Const(c) => {
                    if Self::expr_has_bare_ident(&c.value, var) {
                        return true;
                    }
                }
                Stmt::Assign { target, value, .. } => {
                    if Self::expr_has_bare_ident(target, var) || Self::expr_has_bare_ident(value, var) {
                        return true;
                    }
                }
                Stmt::Return { value, .. } => {
                    if let Some(v) = value {
                        if Self::expr_has_bare_ident(v, var) {
                            return true;
                        }
                    }
                }
                Stmt::Defer { body, .. }
                | Stmt::Region { body, .. }
                | Stmt::Unsafe { body, .. }
                | Stmt::Loop { body, .. } => {
                    if Self::block_has_bare_ident(body, var) {
                        return true;
                    }
                }
                Stmt::While { cond, body, .. } => {
                    if Self::expr_has_bare_ident(cond, var) || Self::block_has_bare_ident(body, var) {
                        return true;
                    }
                }
                Stmt::For { iter, body, .. } => {
                    if Self::expr_has_bare_ident(iter, var) || Self::block_has_bare_ident(body, var) {
                        return true;
                    }
                }
                Stmt::Expr { expr, .. } => {
                    if Self::expr_has_bare_ident(expr, var) {
                        return true;
                    }
                }
                Stmt::Break { .. } | Stmt::Continue { .. } => {}
            }
        }
        false
    }

    fn var_escapes_fn(&self, var: &str, body: &Block) -> bool {
        Self::block_has_bare_ident(body, var)
    }

    fn intern_string(&mut self, s: &str) -> String {
        if let Some((id, _)) = self.string_map.get(s) {
            return format!("@.str.{}", id);
        }
        let id = self.string_pool.len();
        let byte_len = s.as_bytes().len() + 1;
        self.string_pool.push(s.to_string());
        self.string_map.insert(s.to_string(), (id, byte_len));
        format!("@.str.{}", id)
    }

    fn llvm_escape_bytes(s: &str) -> String {
        let mut out = String::new();
        for b in s.as_bytes() {
            if *b >= 32 && *b < 127 && *b != b'"' && *b != b'\\' {
                out.push(*b as char);
            } else {
                out.push_str(&format!("\\{:02X}", b));
            }
        }
        out.push_str("\\00");
        out
    }

    fn llvm_val_type(ty: &Type) -> &'static str {
        match ty.strip_region() {
            Type::Void | Type::Never => "void",
            Type::F32 | Type::F64 => "double",
            Type::String
            | Type::Str
            | Type::Struct(_, _)
            | Type::Enum(_, _)
            | Type::Array(_, _)
            | Type::Vec(_)
            | Type::HashMap(_, _)
            | Type::HashSet(_)
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::RegionArena(_)
            | Type::Shared(_)
            | Type::Weak(_)
            | Type::Mutex(_)
            | Type::Channel(_)
            | Type::Task(_)
            | Type::Pointer(_) => "ptr",
            _ => "i64",
        }
    }

    pub fn generate_llvm_ir(&mut self, module: &ApsIrModule) -> String {
        self.ir_header.clear();
        self.ir_globals.clear();
        self.ir_functions.clear();

        self.ir_header
            .push_str("; ModuleID = 'aps_module'\nsource_filename = \"aps_module.aps\"\n");
        self.ir_header
            .push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));

        // Emit runtime declarations
        self.ir_header.push_str(
            r#"; APS Native Runtime Declarations
declare void @aps_print_i64(i64)
declare void @aps_println_i64(i64)
declare void @aps_print_f64(double)
declare void @aps_println_f64(double)
declare void @aps_print_bool(i64)
declare void @aps_println_bool(i64)
declare void @aps_print_str(ptr)
declare void @aps_println_str(ptr)
declare void @aps_eprintln_str(ptr)
declare void @aps_panic(ptr, ptr, i64) cold noreturn
declare void @aps_bounds_check(i64, i64, ptr, i64) cold noreturn
declare void @aps_assert(i64, ptr)
declare noalias ptr @aps_alloc(i64) willreturn
declare void @aps_free(ptr)
declare void @aps_vec_push(ptr, i64)
declare i64 @aps_vec_pop(ptr)
declare ptr @aps_region_begin(ptr, i64)
declare noalias ptr @aps_region_alloc(ptr, i64) willreturn
declare void @aps_region_end(ptr)
declare i64 @aps_str_len(ptr) readonly willreturn nounwind
declare i64 @aps_str_eq(ptr, ptr) readonly willreturn nounwind
declare noalias ptr @aps_str_concat(ptr, ptr) willreturn
declare ptr @aps_int_to_str(i64) willreturn
declare i64 @aps_time_now_ms()
declare i64 @aps_time_now_ns()
declare void @aps_sleep_ms(i64)
declare i64 @async_sleep_ms(i64)
declare i64 @block_on(i64)
declare i64 @aps_abs(i64) readnone willreturn nounwind
declare i64 @aps_min(i64, i64) readnone willreturn nounwind
declare i64 @aps_max(i64, i64) readnone willreturn nounwind
declare i64 @aps_tcp_listen(i64)
declare i64 @aps_tcp_accept_http(i64, ptr)
declare void @aps_tcp_close(i64)

;Tagged union / Enum standard layout: { i64 tag, i64 payload0, i64 payload1 }
%aps.Enum = type { i64, i64, i64 }
;Dynamic array / Vec layout: { i64 len, i64 cap, ptr data }
%aps.Vec = type { i64, i64, ptr }

"#,
        );

        // Register structs and emit LLVM struct types
        for st in &module.structs {
            self.structs.insert(st.name.clone(), st.fields.clone());
            self.emit_struct_type(st);
        }

        // Register enums
        for en in &module.enums {
            self.enums.insert(en.name.clone(), en.variants.clone());
        }

        // Register functions
        for f in &module.functions {
            self.fn_ret_types.insert(f.name.clone(), f.ret_type.clone());
            let pcaps = f
                .params
                .iter()
                .map(|(_, c, t, _)| (*c, t.clone()))
                .collect();
            self.fn_param_caps.insert(f.name.clone(), pcaps);
        }

        // Emit functions
        for f in &module.functions {
            let fn_ir = self.emit_function(f, &module.consts);
            self.ir_functions.push_str(&fn_ir);
            self.ir_functions.push('\n');
        }

        // If module has no `main`, emit a stub `main` so standalone checks/tests can still link if requested
        if !module.functions.iter().any(|f| f.name == "main") {
            self.ir_functions
                .push_str("define dso_local i32 @main() {\nentry:\n  ret i32 0\n}\n");
        }

        // Emit interned string constants
        for (idx, s) in self.string_pool.iter().enumerate() {
            let byte_len = s.as_bytes().len() + 1;
            let escaped = Self::llvm_escape_bytes(s);
            self.ir_globals.push_str(&format!(
                "@.str.{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n",
                idx, byte_len, escaped
            ));
        }

        format!(
            "{}\n{}\n{}",
            self.ir_header, self.ir_globals, self.ir_functions
        )
    }

    fn emit_struct_type(&mut self, st: &HirStruct) {
        if st.fields.is_empty() {
            self.ir_header
                .push_str(&format!("%struct.{} = type {{ i64 }}\n", st.name));
        } else {
            let f_tys: Vec<&str> = st
                .fields
                .iter()
                .map(|(_, t)| Self::llvm_val_type(t))
                .collect();
            self.ir_header.push_str(&format!(
                "%struct.{} = type {{ {} }}\n",
                st.name,
                f_tys.join(", ")
            ));
        }
    }

    fn emit_function(
        &mut self,
        f: &ApsIrFunction,
        module_consts: &[(String, Type, Expr)],
    ) -> String {
        self.reg_counter = 0;
        self.entry_allocas.clear();
        self.current_fn_body = Some(f.body.clone());
        let mut fn_body = String::new();
        let is_main = f.name == "main";
        let ret_llvm = if is_main {
            "i32"
        } else {
            Self::llvm_val_type(&f.ret_type)
        };

        let mut param_sigs = Vec::new();
        for (pname, cap, pty, _) in &f.params {
            let llvm_ty = if *cap == Capability::Write && pty.is_copy() {
                "ptr"
            } else {
                Self::llvm_val_type(pty)
            };
            let attrs = if *cap == Capability::Write {
                " noalias nocapture "
            } else if *cap == Capability::Read && matches!(pty.strip_region(), Type::Struct(_, _) | Type::Array(_, _) | Type::Vec(_)) {
                " readonly nocapture "
            } else {
                " "
            };
            param_sigs.push(format!("{}{}%arg.{}", llvm_ty, attrs, pname));
        }

        let inline_attr = if f.inline_hint && !is_main {
            " inlinehint"
        } else {
            ""
        };

        let mut locals: HashMap<String, LocalVar> = HashMap::new();

        // Seed module constants
        for (cname, cty, cexpr) in module_consts {
            let (cval, actual_ty) = self.emit_expr(cexpr, &mut fn_body, &mut locals, Some(cty));
            let lty = Self::llvm_val_type(&actual_ty);
            let slot = self.emit_alloca(lty);
            fn_body.push_str(&format!("  store {} {}, ptr {}, align 8\n", lty, cval, slot));
            locals.insert(
                cname.clone(),
                LocalVar {
                    alloca_reg: slot,
                    ty: actual_ty,
                    is_write_ref: false,
                },
            );
        }

        // Allocate stack slots for parameters
        for (pname, cap, pty, _) in &f.params {
            if *cap == Capability::Write && pty.is_copy() {
                // Directly use the pointer passed by caller
                locals.insert(
                    pname.clone(),
                    LocalVar {
                        alloca_reg: format!("%arg.{}", pname),
                        ty: pty.clone(),
                        is_write_ref: true,
                    },
                );
            } else {
                let lty = Self::llvm_val_type(pty);
                let slot = self.emit_alloca(lty);
                fn_body.push_str(&format!(
                    "  store {} %arg.{}, ptr {}, align 8\n",
                    lty, pname, slot
                ));
                locals.insert(
                    pname.clone(),
                    LocalVar {
                        alloca_reg: slot,
                        ty: pty.clone(),
                        is_write_ref: false,
                    },
                );
            }
        }

        let mut defer_stack: Vec<Block> = Vec::new();
        let mut loop_stack: Vec<(String, String)> = Vec::new();
        let (last_val, last_ty, did_return) = self.emit_block(
            &f.body,
            &mut fn_body,
            &mut locals,
            &mut defer_stack,
            &mut loop_stack,
            is_main,
            &f.ret_type,
        );

        if !did_return {
            // Execute any remaining defers in reverse order
            for dblk in defer_stack.iter().rev() {
                let mut empty_defers = Vec::new();
                let _ = self.emit_block(
                    dblk,
                    &mut fn_body,
                    &mut locals,
                    &mut empty_defers,
                    &mut loop_stack,
                    is_main,
                    &f.ret_type,
                );
            }
            if is_main {
                fn_body.push_str("  ret i32 0\n");
            } else if f.ret_type == Type::Void || f.ret_type == Type::Never {
                fn_body.push_str("  ret void\n");
            } else {
                let rty = Self::llvm_val_type(&f.ret_type);
                if let Some(v) = last_val {
                    if Self::llvm_val_type(&last_ty) == rty {
                        fn_body.push_str(&format!("  ret {} {}\n", rty, v));
                    } else if rty == "ptr" {
                        fn_body.push_str("  ret ptr null\n");
                    } else if rty == "double" {
                        fn_body.push_str("  ret double 0.0\n");
                    } else {
                        fn_body.push_str("  ret i64 0\n");
                    }
                } else if rty == "ptr" {
                    fn_body.push_str("  ret ptr null\n");
                } else if rty == "double" {
                    fn_body.push_str("  ret double 0.0\n");
                } else {
                    fn_body.push_str("  ret i64 0\n");
                }
            }
        }

        let mut out = String::new();
        out.push_str(&format!(
            "define dso_local {} @{}({}){} {{\nentry:\n",
            ret_llvm,
            Self::sanitize_ident(&f.name),
            param_sigs.join(", "),
            inline_attr
        ));
        out.push_str(&self.entry_allocas);
        out.push_str(&fn_body);
        out.push_str("}\n");
        self.current_fn_body = None;
        out
    }

    fn sanitize_ident(name: &str) -> String {
        name.replace("::", "__").replace('.', "__")
    }

    fn emit_block(
        &mut self,
        block: &Block,
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
        defer_stack: &mut Vec<Block>,
        loop_stack: &mut Vec<(String, String)>,
        is_main: bool,
        fn_ret_type: &Type,
    ) -> (Option<String>, Type, bool) {
        let saved_defer_len = defer_stack.len();
        let mut last_val = None;
        let mut last_ty = Type::Void;

        for (idx, stmt) in block.stmts.iter().enumerate() {
            let is_last = idx + 1 == block.stmts.len();
            match stmt {
                Stmt::Let { name, ty, init, .. } | Stmt::Var { name, ty, init, .. } => {
                    let is_stack_struct = if let ExprKind::StructInit { name: sname, .. } = &init.kind {
                        let is_copy = self.is_struct_all_copy(sname);
                        let escapes = if let Some(ref b) = self.current_fn_body {
                            self.var_escapes_fn(name, b)
                        } else {
                            true
                        };
                        is_copy && !escapes
                    } else {
                        false
                    };

                    if is_stack_struct {
                        if let ExprKind::StructInit { name: sname, fields } = &init.kind {
                            let sptr = self.emit_alloca(&format!("%struct.{}", sname));
                            let sfields = self.structs.get(sname).cloned().unwrap_or_default();
                            for (fname, fexpr) in fields {
                                if let Some((idx, (_, fty))) =
                                    sfields.iter().enumerate().find(|(_, (n, _))| n == fname)
                                {
                                    let (fval, _) = self.emit_expr(fexpr, out, locals, Some(fty));
                                    let gep = self.next_reg();
                                    out.push_str(&format!(
                                        "  {} = getelementptr inbounds %struct.{}, ptr {}, i32 0, i32 {}\n",
                                        gep, sname, sptr, idx
                                    ));
                                    let flty = Self::llvm_val_type(fty);
                                    out.push_str(&format!(
                                        "  store {} {}, ptr {}, align 8\n",
                                        flty, fval, gep
                                    ));
                                }
                            }
                            let slot = self.emit_alloca("ptr");
                            out.push_str(&format!("  store ptr {}, ptr {}, align 8\n", sptr, slot));
                            locals.insert(
                                name.clone(),
                                LocalVar {
                                    alloca_reg: slot,
                                    ty: Type::Struct(sname.clone(), Vec::new()),
                                    is_write_ref: false,
                                },
                            );
                        }
                    } else {
                        let expected = self.resolve_basic_type_ast(ty);
                        let (val, inferred_ty) =
                            self.emit_expr(init, out, locals, expected.as_ref());
                        let final_ty = expected.unwrap_or(inferred_ty);
                        let lty = Self::llvm_val_type(&final_ty);
                        let slot = self.emit_alloca(lty);
                        out.push_str(&format!("  store {} {}, ptr {}, align 8\n", lty, val, slot));
                        locals.insert(
                            name.clone(),
                            LocalVar {
                                alloca_reg: slot,
                                ty: final_ty,
                                is_write_ref: false,
                            },
                        );
                    }
                }
                Stmt::Const(c) => {
                    let expected = self.resolve_basic_type_ast(&c.ty);
                    let (val, inferred_ty) =
                        self.emit_expr(&c.value, out, locals, expected.as_ref());
                    let final_ty = expected.unwrap_or(inferred_ty);
                    let lty = Self::llvm_val_type(&final_ty);
                    let slot = self.emit_alloca(lty);
                    out.push_str(&format!("  store {} {}, ptr {}, align 8\n", lty, val, slot));
                    locals.insert(
                        c.name.clone(),
                        LocalVar {
                            alloca_reg: slot,
                            ty: final_ty,
                            is_write_ref: false,
                        },
                    );
                }
                Stmt::Assign {
                    target, op, value, ..
                } => {
                    self.emit_assignment(target, *op, value, out, locals);
                }
                Stmt::Defer { body, .. } => {
                    defer_stack.push(body.clone());
                }
                Stmt::Region { name, body, .. } => {
                    let rname_global = self.intern_string(name);
                    let reg_ptr = self.next_reg();
                    out.push_str(&format!(
                        "  {} = call ptr @aps_region_begin(ptr {}, i64 65536)\n",
                        reg_ptr, rname_global
                    ));
                    let slot = self.emit_alloca("ptr");
                    out.push_str(&format!("  store ptr {}, ptr {}, align 8\n", reg_ptr, slot));
                    locals.insert(
                        name.clone(),
                        LocalVar {
                            alloca_reg: slot,
                            ty: Type::RegionArena(name.clone()),
                            is_write_ref: false,
                        },
                    );
                    let (_, _, did_ret) = self.emit_block(
                        body,
                        out,
                        locals,
                        defer_stack,
                        loop_stack,
                        is_main,
                        fn_ret_type,
                    );
                    if !did_ret {
                        out.push_str(&format!("  call void @aps_region_end(ptr {})\n", reg_ptr));
                    } else {
                        return (None, Type::Never, true);
                    }
                }
                Stmt::Unsafe { body, .. } => {
                    let (v, t, did_ret) = self.emit_block(
                        body,
                        out,
                        locals,
                        defer_stack,
                        loop_stack,
                        is_main,
                        fn_ret_type,
                    );
                    if did_ret {
                        return (v, t, true);
                    }
                    if is_last {
                        last_val = v;
                        last_ty = t;
                    }
                }
                Stmt::Return { value, .. } => {
                    let ret_pair = value
                        .as_ref()
                        .map(|e| self.emit_expr(e, out, locals, Some(fn_ret_type)));
                    // Run all active defers before returning
                    for dblk in defer_stack.iter().rev() {
                        let mut tmp_defers = Vec::new();
                        let _ = self.emit_block(
                            dblk,
                            out,
                            locals,
                            &mut tmp_defers,
                            loop_stack,
                            is_main,
                            fn_ret_type,
                        );
                    }
                    if is_main {
                        out.push_str("  ret i32 0\n");
                    } else if let Some((v, _)) = ret_pair {
                        let rty = Self::llvm_val_type(fn_ret_type);
                        if rty == "void" {
                            out.push_str("  ret void\n");
                        } else {
                            out.push_str(&format!("  ret {} {}\n", rty, v));
                        }
                    } else {
                        let rty = Self::llvm_val_type(fn_ret_type);
                        if rty == "void" {
                            out.push_str("  ret void\n");
                        } else if rty == "i32" {
                            out.push_str("  ret i32 0\n");
                        } else if rty == "i64" {
                            out.push_str("  ret i64 0\n");
                        } else {
                            out.push_str("  ret void\n");
                        }
                    }
                    return (None, Type::Never, true);
                }
                Stmt::Break { .. } => {
                    if let Some((_, break_lbl)) = loop_stack.last() {
                        out.push_str(&format!("  br label %{}\n", break_lbl));
                        return (None, Type::Never, true);
                    }
                }
                Stmt::Continue { .. } => {
                    if let Some((cond_lbl, _)) = loop_stack.last() {
                        out.push_str(&format!("  br label %{}\n", cond_lbl));
                        return (None, Type::Never, true);
                    }
                }
                Stmt::While { cond, body, .. } => {
                    let cond_lbl = self.next_label("while_cond");
                    let body_lbl = self.next_label("while_body");
                    let end_lbl = self.next_label("while_end");

                    out.push_str(&format!("  br label %{}\n", cond_lbl));
                    out.push_str(&format!("{}:\n", cond_lbl));
                    let (cond_val, _) = self.emit_expr(cond, out, locals, Some(&Type::Bool));
                    let cond_i1 = self.next_reg();
                    out.push_str(&format!("  {} = icmp ne i64 {}, 0\n", cond_i1, cond_val));
                    out.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n",
                        cond_i1, body_lbl, end_lbl
                    ));

                    out.push_str(&format!("{}:\n", body_lbl));
                    loop_stack.push((cond_lbl.clone(), end_lbl.clone()));
                    let (_, _, body_terminated) = self.emit_block(
                        body,
                        out,
                        locals,
                        defer_stack,
                        loop_stack,
                        is_main,
                        fn_ret_type,
                    );
                    loop_stack.pop();
                    if !body_terminated {
                        out.push_str(&format!("  br label %{}\n", cond_lbl));
                    }
                    out.push_str(&format!("{}:\n", end_lbl));
                }
                Stmt::Loop { body, .. } => {
                    let body_lbl = self.next_label("loop_body");
                    let end_lbl = self.next_label("loop_end");
                    out.push_str(&format!("  br label %{}\n", body_lbl));
                    out.push_str(&format!("{}:\n", body_lbl));
                    loop_stack.push((body_lbl.clone(), end_lbl.clone()));
                    let (_, _, body_terminated) = self.emit_block(
                        body,
                        out,
                        locals,
                        defer_stack,
                        loop_stack,
                        is_main,
                        fn_ret_type,
                    );
                    loop_stack.pop();
                    if !body_terminated {
                        out.push_str(&format!("  br label %{}\n", body_lbl));
                    }
                    out.push_str(&format!("{}:\n", end_lbl));
                }
                Stmt::For {
                    var, iter, body, ..
                } => {
                    // Fallback for array/vec iteration `for item in arr`
                    let (arr_ptr, arr_ty) = self.emit_expr(iter, out, locals, None);
                    let elem_ty = match arr_ty {
                        Type::Array(e, _) | Type::Vec(e) => *e,
                        _ => Type::I64,
                    };
                    let idx_slot = self.emit_alloca("i64");
                    out.push_str(&format!("  store i64 0, ptr {}, align 8\n", idx_slot));

                    let len_ptr = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 0\n",
                        len_ptr, arr_ptr
                    ));
                    let len_val = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", len_val, len_ptr));

                    let cond_lbl = self.next_label("for_cond");
                    let body_lbl = self.next_label("for_body");
                    let step_lbl = self.next_label("for_step");
                    let end_lbl = self.next_label("for_end");

                    out.push_str(&format!("  br label %{}\n", cond_lbl));
                    out.push_str(&format!("{}:\n", cond_lbl));
                    let cur_idx = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", cur_idx, idx_slot));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp slt i64 {}, {}\n", cmp, cur_idx, len_val));
                    out.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n",
                        cmp, body_lbl, end_lbl
                    ));

                    out.push_str(&format!("{}:\n", body_lbl));
                    let data_field = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                        data_field, arr_ptr
                    ));
                    let data_ptr = self.next_reg();
                    out.push_str(&format!(
                        "  {} = load ptr, ptr {}, align 8\n",
                        data_ptr, data_field
                    ));
                    let elem_llvm = Self::llvm_val_type(&elem_ty);
                    let elem_ptr = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds {}, ptr {}, i64 {}\n",
                        elem_ptr, elem_llvm, data_ptr, cur_idx
                    ));
                    let elem_val = self.next_reg();
                    out.push_str(&format!(
                        "  {} = load {}, ptr {}, align 8\n",
                        elem_val, elem_llvm, elem_ptr
                    ));

                    let var_slot = self.emit_alloca(elem_llvm);
                    out.push_str(&format!(
                        "  store {} {}, ptr {}, align 8\n",
                        elem_llvm, elem_val, var_slot
                    ));
                    locals.insert(
                        var.clone(),
                        LocalVar {
                            alloca_reg: var_slot,
                            ty: elem_ty,
                            is_write_ref: false,
                        },
                    );

                    loop_stack.push((step_lbl.clone(), end_lbl.clone()));
                    let (_, _, term) = self.emit_block(
                        body,
                        out,
                        locals,
                        defer_stack,
                        loop_stack,
                        is_main,
                        fn_ret_type,
                    );
                    loop_stack.pop();
                    if !term {
                        out.push_str(&format!("  br label %{}\n", step_lbl));
                    }
                    out.push_str(&format!("{}:\n", step_lbl));
                    let idx_now = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", idx_now, idx_slot));
                    let idx_next = self.next_reg();
                    out.push_str(&format!("  {} = add i64 {}, 1\n", idx_next, idx_now));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", idx_next, idx_slot));
                    out.push_str(&format!("  br label %{}\n", cond_lbl));
                    out.push_str(&format!("{}:\n", end_lbl));
                }
                Stmt::Expr { expr, .. } => {
                    let (v, t) = self.emit_expr(expr, out, locals, None);
                    if is_last {
                        last_val = Some(v);
                        last_ty = t;
                    }
                }
            }
        }

        // Execute block-local defers in LIFO order
        while defer_stack.len() > saved_defer_len {
            if let Some(dblk) = defer_stack.pop() {
                let _ = self.emit_block(
                    &dblk,
                    out,
                    locals,
                    defer_stack,
                    loop_stack,
                    is_main,
                    fn_ret_type,
                );
            }
        }

        (last_val, last_ty, false)
    }

    fn emit_assignment(
        &mut self,
        target: &Expr,
        op: AssignOp,
        value: &Expr,
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
    ) {
        let (rhs_val, rhs_ty) = self.emit_expr(value, out, locals, None);
        if let Some((ptr_reg, target_ty)) = self.emit_lvalue_ptr(target, out, locals) {
            let lty = Self::llvm_val_type(&target_ty);
            let final_val = if op == AssignOp::Assign {
                rhs_val
            } else {
                let cur_val = self.next_reg();
                out.push_str(&format!("  {} = load {}, ptr {}, align 8\n", cur_val, lty, ptr_reg));
                let res = self.next_reg();
                if lty == "double" {
                    let fop = match op {
                        AssignOp::AddAssign => "fadd",
                        AssignOp::SubAssign => "fsub",
                        AssignOp::MulAssign => "fmul",
                        AssignOp::DivAssign => "fdiv",
                        _ => "fadd",
                    };
                    out.push_str(&format!("  {} = {} double {}, {}\n", res, fop, cur_val, rhs_val));
                } else {
                    if matches!(op, AssignOp::DivAssign | AssignOp::RemAssign) {
                        let is_known_non_zero = match rhs_val.parse::<i64>() {
                            Ok(c) => c != 0,
                            Err(_) => false,
                        };
                        if !is_known_non_zero {
                            let is_zero = self.next_reg();
                            let div_ok = self.next_label("div_ok");
                            let div_fail = self.next_label("div_fail");
                            let err_str = self.intern_string("attempt to divide by zero in compound assignment");
                            let file_str = self.intern_string(&target.span.file);
                            out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_zero, rhs_val));
                            out.push_str(&format!("  br i1 {}, label %{}, label %{}\n", is_zero, div_fail, div_ok));
                            out.push_str(&format!("{}:\n", div_fail));
                            out.push_str(&format!(
                                "  call void @aps_panic(ptr {}, ptr {}, i64 {})\n  unreachable\n",
                                err_str, file_str, target.span.line
                            ));
                            out.push_str(&format!("{}:\n", div_ok));
                        }
                    }
                    let iop = match op {
                        AssignOp::AddAssign => "add",
                        AssignOp::SubAssign => "sub",
                        AssignOp::MulAssign => "mul",
                        AssignOp::DivAssign => "sdiv",
                        AssignOp::RemAssign => "srem",
                        AssignOp::Assign => "add",
                    };
                    out.push_str(&format!("  {} = {} i64 {}, {}\n", res, iop, cur_val, rhs_val));
                }
                res
            };
            let store_lty = Self::llvm_val_type(&rhs_ty);
            let effective_lty = if lty == store_lty { lty } else { store_lty };
            out.push_str(&format!(
                "  store {} {}, ptr {}, align 8\n",
                effective_lty, final_val, ptr_reg
            ));
        }
    }

    fn emit_lvalue_ptr(
        &mut self,
        target: &Expr,
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
    ) -> Option<(String, Type)> {
        match &target.kind {
            ExprKind::Ident(name) => {
                let lv = locals.get(name)?;
                Some((lv.alloca_reg.clone(), lv.ty.clone()))
            }
            ExprKind::FieldAccess { base, field } => {
                let (base_ptr, base_ty) = self.emit_expr(base, out, locals, None);
                if let Type::Struct(sname, _) = base_ty.strip_region() {
                    if let Some(fields) = self.structs.get(sname).cloned() {
                        if let Some((idx, (_, fty))) =
                            fields.iter().enumerate().find(|(_, (fnm, _))| fnm == field)
                        {
                            let gep = self.next_reg();
                            out.push_str(&format!(
                                "  {} = getelementptr inbounds %struct.{}, ptr {}, i32 0, i32 {}\n",
                                gep, sname, base_ptr, idx
                            ));
                            return Some((gep, fty.clone()));
                        }
                    }
                }
                None
            }
            ExprKind::Index { base, index } => {
                let (vec_ptr, base_ty) = self.emit_expr(base, out, locals, None);
                let (idx_val, _) = self.emit_expr(index, out, locals, Some(&Type::I64));
                let elem_ty = match base_ty.strip_region() {
                    Type::Array(e, _) | Type::Vec(e) => (**e).clone(),
                    _ => Type::I64,
                };
                let len_p = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 0\n",
                    len_p, vec_ptr
                ));
                let len_v = self.next_reg();
                out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", len_v, len_p));
                let is_ok = self.next_reg();
                let ok_lbl = self.next_label("bounds_ok");
                let fail_lbl = self.next_label("bounds_fail");
                let file_str = self.intern_string(&target.span.file);
                out.push_str(&format!("  {} = icmp ult i64 {}, {}\n", is_ok, idx_val, len_v));
                out.push_str(&format!("  br i1 {}, label %{}, label %{}\n", is_ok, ok_lbl, fail_lbl));
                out.push_str(&format!("{}:\n", fail_lbl));
                out.push_str(&format!(
                    "  call void @aps_bounds_check(i64 {}, i64 {}, ptr {}, i64 {})\n  unreachable\n",
                    idx_val, len_v, file_str, target.span.line
                ));
                out.push_str(&format!("{}:\n", ok_lbl));
                let data_field = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                    data_field, vec_ptr
                ));
                let data_ptr = self.next_reg();
                out.push_str(&format!(
                    "  {} = load ptr, ptr {}, align 8\n",
                    data_ptr, data_field
                ));
                let elem_llvm = Self::llvm_val_type(&elem_ty);
                let elem_ptr = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds {}, ptr {}, i64 {}\n",
                    elem_ptr, elem_llvm, data_ptr, idx_val
                ));
                Some((elem_ptr, elem_ty))
            }
            _ => None,
        }
    }

    fn resolve_basic_type_ast(&self, ast: &TypeAst) -> Option<Type> {
        match ast {
            TypeAst::Inferred => None,
            TypeAst::Void => Some(Type::Void),
            TypeAst::Capability { inner, .. } | TypeAst::Shared(inner) => {
                self.resolve_basic_type_ast(inner)
            }
            TypeAst::Array { elem, size } => {
                let inner = self.resolve_basic_type_ast(elem).unwrap_or(Type::I64);
                Some(Type::Array(Box::new(inner), *size))
            }
            TypeAst::Named { name, args } => match name.as_str() {
                "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "usize" => Some(Type::I64),
                "f32" | "f64" => Some(Type::F64),
                "bool" => Some(Type::Bool),
                "char" => Some(Type::Char),
                "String" | "str" => Some(Type::String),
                "Vec" => {
                    let inner = args
                        .first()
                        .and_then(|a| self.resolve_basic_type_ast(a))
                        .unwrap_or(Type::I64);
                    Some(Type::Vec(Box::new(inner)))
                }
                "Option" => {
                    let inner = args
                        .first()
                        .and_then(|a| self.resolve_basic_type_ast(a))
                        .unwrap_or(Type::I64);
                    Some(Type::Option(Box::new(inner)))
                }
                "Result" => {
                    let ok = args
                        .first()
                        .and_then(|a| self.resolve_basic_type_ast(a))
                        .unwrap_or(Type::I64);
                    Some(Type::Result(Box::new(ok), Box::new(Type::String)))
                }
                other => {
                    if self.enums.contains_key(other) {
                        Some(Type::Enum(other.to_string(), Vec::new()))
                    } else {
                        Some(Type::Struct(other.to_string(), Vec::new()))
                    }
                }
            },
            _ => None,
        }
    }

    fn emit_expr(
        &mut self,
        expr: &Expr,
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
        expected_ty: Option<&Type>,
    ) -> (String, Type) {
        match &expr.kind {
            ExprKind::IntLit(v, _) => {
                if matches!(expected_ty, Some(Type::F32 | Type::F64)) {
                    (format!("{:.6e}", *v as f64), Type::F64)
                } else {
                    (v.to_string(), Type::I64)
                }
            }
            ExprKind::FloatLit(v, _) => (format!("{:.6e}", v), Type::F64),
            ExprKind::BoolLit(b) => ((if *b { "1" } else { "0" }).to_string(), Type::Bool),
            ExprKind::CharLit(c) => ((*c as u32 as i64).to_string(), Type::Char),
            ExprKind::StringLit(s) => {
                let sym = self.intern_string(s);
                (sym, Type::String)
            }
            ExprKind::Ident(name) => {
                if let Some(lv) = locals.get(name).cloned() {
                    let lty = Self::llvm_val_type(&lv.ty);
                    let reg = self.next_reg();
                    out.push_str(&format!(
                        "  {} = load {}, ptr {}, align 8\n",
                        reg, lty, lv.alloca_reg
                    ));
                    return (reg, lv.ty);
                }
                if name == "None" {
                    let enum_ptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", enum_ptr));
                    return (enum_ptr, Type::Option(Box::new(Type::I64)));
                }
                // Check unit enum variant `Status::Idle` or `Idle`
                if let Some((ename, vname)) = name.split_once("::") {
                    if let Some(variants) = self.enums.get(ename).cloned() {
                        let tag = variants
                            .iter()
                            .position(|(vn, _)| vn == vname)
                            .unwrap_or(0) as i64;
                        let eptr = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", eptr));
                        let tag_ptr = self.next_reg();
                        out.push_str(&format!(
                            "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n",
                            tag_ptr, eptr
                        ));
                        out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", tag, tag_ptr));
                        return (eptr, Type::Enum(ename.to_string(), Vec::new()));
                    }
                }
                ("0".to_string(), Type::I64)
            }
            ExprKind::Unary { op, expr: inner } => match op {
                UnaryOp::Neg => {
                    let (v, ty) = self.emit_expr(inner, out, locals, expected_ty);
                    let r = self.next_reg();
                    if ty.is_float() {
                        out.push_str(&format!("  {} = fneg double {}\n", r, v));
                    } else {
                        out.push_str(&format!("  {} = sub i64 0, {}\n", r, v));
                    }
                    (r, ty)
                }
                UnaryOp::Not => {
                    let (v, _) = self.emit_expr(inner, out, locals, Some(&Type::Bool));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", cmp, v));
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = zext i1 {} to i64\n", r, cmp));
                    (r, Type::Bool)
                }
                UnaryOp::BitNot => {
                    let (v, ty) = self.emit_expr(inner, out, locals, None);
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = xor i64 {}, -1\n", r, v));
                    (r, ty)
                }
                UnaryOp::Read | UnaryOp::Write | UnaryOp::Take | UnaryOp::Share | UnaryOp::Await
                | UnaryOp::Spawn => self.emit_expr(inner, out, locals, expected_ty),
            },
            ExprKind::Cast { expr: inner, ty } => {
                let (v, src_ty) = self.emit_expr(inner, out, locals, None);
                let dst_ty = self.resolve_basic_type_ast(ty).unwrap_or(Type::I64);
                if src_ty.is_float() && dst_ty.is_integer() {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = fptosi double {} to i64\n", r, v));
                    (r, dst_ty)
                } else if src_ty.is_integer() && dst_ty.is_float() {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = sitofp i64 {} to double\n", r, v));
                    (r, dst_ty)
                } else {
                    (v, dst_ty)
                }
            }
            ExprKind::Binary { op, left, right } => {
                let (lv, lty) = self.emit_expr(left, out, locals, None);
                let (rv, rty) = self.emit_expr(right, out, locals, Some(&lty));

                // String concatenation or equality
                if matches!(lty, Type::String | Type::Str) || matches!(rty, Type::String | Type::Str)
                {
                    let res = self.next_reg();
                    match op {
                        BinOp::Add => {
                            out.push_str(&format!(
                                "  {} = call ptr @aps_str_concat(ptr {}, ptr {})\n",
                                res, lv, rv
                            ));
                            return (res, Type::String);
                        }
                        BinOp::Eq => {
                            out.push_str(&format!(
                                "  {} = call i64 @aps_str_eq(ptr {}, ptr {})\n",
                                res, lv, rv
                            ));
                            return (res, Type::Bool);
                        }
                        BinOp::Ne => {
                            let eq = self.next_reg();
                            out.push_str(&format!(
                                "  {} = call i64 @aps_str_eq(ptr {}, ptr {})\n",
                                eq, lv, rv
                            ));
                            out.push_str(&format!("  {} = xor i64 {}, 1\n", res, eq));
                            return (res, Type::Bool);
                        }
                        _ => {}
                    }
                }

                if lty.is_float() || rty.is_float() {
                    let res = self.next_reg();
                    match op {
                        BinOp::Add => {
                            out.push_str(&format!("  {} = fadd double {}, {}\n", res, lv, rv));
                            (res, Type::F64)
                        }
                        BinOp::Sub => {
                            out.push_str(&format!("  {} = fsub double {}, {}\n", res, lv, rv));
                            (res, Type::F64)
                        }
                        BinOp::Mul => {
                            out.push_str(&format!("  {} = fmul double {}, {}\n", res, lv, rv));
                            (res, Type::F64)
                        }
                        BinOp::Div => {
                            out.push_str(&format!("  {} = fdiv double {}, {}\n", res, lv, rv));
                            (res, Type::F64)
                        }
                        BinOp::Rem => {
                            out.push_str(&format!("  {} = frem double {}, {}\n", res, lv, rv));
                            (res, Type::F64)
                        }
                        _ => {
                            let cmp_op = match op {
                                BinOp::Eq => "oeq",
                                BinOp::Ne => "une",
                                BinOp::Lt => "olt",
                                BinOp::Le => "ole",
                                BinOp::Gt => "ogt",
                                BinOp::Ge => "oge",
                                _ => "oeq",
                            };
                            let cmp_i1 = self.next_reg();
                            out.push_str(&format!(
                                "  {} = fcmp {} double {}, {}\n",
                                cmp_i1, cmp_op, lv, rv
                            ));
                            out.push_str(&format!("  {} = zext i1 {} to i64\n", res, cmp_i1));
                            (res, Type::Bool)
                        }
                    }
                } else {
                    let res = self.next_reg();
                    match op {
                        BinOp::Add => {
                            out.push_str(&format!("  {} = add i64 {}, {}\n", res, lv, rv));
                            (res, Type::I64)
                        }
                        BinOp::Sub => {
                            out.push_str(&format!("  {} = sub i64 {}, {}\n", res, lv, rv));
                            (res, Type::I64)
                        }
                        BinOp::Mul => {
                            out.push_str(&format!("  {} = mul i64 {}, {}\n", res, lv, rv));
                            (res, Type::I64)
                        }
                        BinOp::Div => {
                            let is_known_non_zero = match rv.parse::<i64>() {
                                Ok(c) => c != 0,
                                Err(_) => false,
                            };
                            if is_known_non_zero {
                                out.push_str(&format!("  {} = sdiv i64 {}, {}\n", res, lv, rv));
                            } else {
                                let is_zero = self.next_reg();
                                let div_ok = self.next_label("div_ok");
                                let div_fail = self.next_label("div_fail");
                                let err_str = self.intern_string("attempt to divide by zero");
                                let file_str = self.intern_string(&expr.span.file);
                                out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_zero, rv));
                                out.push_str(&format!("  br i1 {}, label %{}, label %{}\n", is_zero, div_fail, div_ok));
                                out.push_str(&format!("{}:\n", div_fail));
                                out.push_str(&format!(
                                    "  call void @aps_panic(ptr {}, ptr {}, i64 {})\n  unreachable\n",
                                    err_str, file_str, expr.span.line
                                ));
                                out.push_str(&format!("{}:\n", div_ok));
                                out.push_str(&format!("  {} = sdiv i64 {}, {}\n", res, lv, rv));
                            }
                            (res, Type::I64)
                        }
                        BinOp::Rem => {
                            let is_known_non_zero = match rv.parse::<i64>() {
                                Ok(c) => c != 0,
                                Err(_) => false,
                            };
                            if is_known_non_zero {
                                out.push_str(&format!("  {} = srem i64 {}, {}\n", res, lv, rv));
                            } else {
                                let is_zero = self.next_reg();
                                let rem_ok = self.next_label("rem_ok");
                                let rem_fail = self.next_label("rem_fail");
                                let err_str = self.intern_string("attempt to calculate remainder with zero divisor");
                                let file_str = self.intern_string(&expr.span.file);
                                out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_zero, rv));
                                out.push_str(&format!("  br i1 {}, label %{}, label %{}\n", is_zero, rem_fail, rem_ok));
                                out.push_str(&format!("{}:\n", rem_fail));
                                out.push_str(&format!(
                                    "  call void @aps_panic(ptr {}, ptr {}, i64 {})\n  unreachable\n",
                                    err_str, file_str, expr.span.line
                                ));
                                out.push_str(&format!("{}:\n", rem_ok));
                                out.push_str(&format!("  {} = srem i64 {}, {}\n", res, lv, rv));
                            }
                            (res, Type::I64)
                        }
                        BinOp::BitAnd | BinOp::And => {
                            out.push_str(&format!("  {} = and i64 {}, {}\n", res, lv, rv));
                            (
                                res,
                                if *op == BinOp::And {
                                    Type::Bool
                                } else {
                                    Type::I64
                                },
                            )
                        }
                        BinOp::BitOr | BinOp::Or => {
                            out.push_str(&format!("  {} = or i64 {}, {}\n", res, lv, rv));
                            (
                                res,
                                if *op == BinOp::Or {
                                    Type::Bool
                                } else {
                                    Type::I64
                                },
                            )
                        }
                        BinOp::BitXor => {
                            out.push_str(&format!("  {} = xor i64 {}, {}\n", res, lv, rv));
                            (res, Type::I64)
                        }
                        BinOp::Shl => {
                            out.push_str(&format!("  {} = shl i64 {}, {}\n", res, lv, rv));
                            (res, Type::I64)
                        }
                        BinOp::Shr => {
                            out.push_str(&format!("  {} = ashr i64 {}, {}\n", res, lv, rv));
                            (res, Type::I64)
                        }
                        _ => {
                            let cmp_op = match op {
                                BinOp::Eq => "eq",
                                BinOp::Ne => "ne",
                                BinOp::Lt => "slt",
                                BinOp::Le => "sle",
                                BinOp::Gt => "sgt",
                                BinOp::Ge => "sge",
                                _ => "eq",
                            };
                            let cmp_i1 = self.next_reg();
                            out.push_str(&format!(
                                "  {} = icmp {} i64 {}, {}\n",
                                cmp_i1, cmp_op, lv, rv
                            ));
                            out.push_str(&format!("  {} = zext i1 {} to i64\n", res, cmp_i1));
                            (res, Type::Bool)
                        }
                    }
                }
            }
            ExprKind::StructInit { name, fields } => {
                let sfields = self.structs.get(name).cloned().unwrap_or_default();
                let byte_size = (sfields.len().max(1) * 8) as i64;
                let sptr = self.next_reg();
                out.push_str(&format!(
                    "  {} = call ptr @aps_alloc(i64 {})\n",
                    sptr, byte_size
                ));
                for (fname, fexpr) in fields {
                    if let Some((idx, (_, fty))) =
                        sfields.iter().enumerate().find(|(_, (n, _))| n == fname)
                    {
                        let (fval, _) = self.emit_expr(fexpr, out, locals, Some(fty));
                        let gep = self.next_reg();
                        out.push_str(&format!(
                            "  {} = getelementptr inbounds %struct.{}, ptr {}, i32 0, i32 {}\n",
                            gep, name, sptr, idx
                        ));
                        let flty = Self::llvm_val_type(fty);
                        out.push_str(&format!(
                            "  store {} {}, ptr {}, align 8\n",
                            flty, fval, gep
                        ));
                    }
                }
                (sptr, Type::Struct(name.clone(), Vec::new()))
            }
            ExprKind::FieldAccess { base, field } => {
                // Check if base is an Enum name like `Status.Idle`
                if let ExprKind::Ident(bname) = &base.kind {
                    if !locals.contains_key(bname) {
                        if let Some(variants) = self.enums.get(bname).cloned() {
                            let tag = variants
                                .iter()
                                .position(|(vn, _)| vn == field)
                                .unwrap_or(0) as i64;
                            let eptr = self.next_reg();
                            out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", eptr));
                            let tag_ptr = self.next_reg();
                            out.push_str(&format!(
                                "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n",
                                tag_ptr, eptr
                            ));
                            out.push_str(&format!(
                                "  store i64 {}, ptr {}, align 8\n",
                                tag, tag_ptr
                            ));
                            return (eptr, Type::Enum(bname.clone(), Vec::new()));
                        }
                    }
                }
                let (base_ptr, base_ty) = self.emit_expr(base, out, locals, None);
                let clean_ty = match base_ty.strip_region() {
                    Type::Shared(inner) => inner.as_ref(),
                    other => other,
                };
                if let Type::Struct(sname, _) = clean_ty {
                    if let Some(sfields) = self.structs.get(sname).cloned() {
                        if let Some((idx, (_, fty))) =
                            sfields.iter().enumerate().find(|(_, (n, _))| n == field)
                        {
                            let gep = self.next_reg();
                            out.push_str(&format!(
                                "  {} = getelementptr inbounds %struct.{}, ptr {}, i32 0, i32 {}\n",
                                gep, sname, base_ptr, idx
                            ));
                            let flty = Self::llvm_val_type(fty);
                            let val = self.next_reg();
                            out.push_str(&format!(
                                "  {} = load {}, ptr {}, align 8\n",
                                val, flty, gep
                            ));
                            return (val, fty.clone());
                        }
                    }
                }
                ("0".to_string(), Type::I64)
            }
            ExprKind::ArrayLit(elems) => {
                let len = elems.len() as i64;
                let cap = len.max(8);
                let mut elem_ty = Type::I64;
                let mut vals = Vec::new();
                for e in elems {
                    let (v, t) = self.emit_expr(e, out, locals, None);
                    elem_ty = t;
                    vals.push(v);
                }
                let elem_llvm = Self::llvm_val_type(&elem_ty);
                let data_ptr = self.next_reg();
                out.push_str(&format!(
                    "  {} = call ptr @aps_alloc(i64 {})\n",
                    data_ptr,
                    cap * 8
                ));
                for (i, v) in vals.into_iter().enumerate() {
                    let ep = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds {}, ptr {}, i64 {}\n",
                        ep, elem_llvm, data_ptr, i
                    ));
                    out.push_str(&format!(
                        "  store {} {}, ptr {}, align 8\n",
                        elem_llvm, v, ep
                    ));
                }
                let vec_ptr = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", vec_ptr));
                let len_p = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 0\n",
                    len_p, vec_ptr
                ));
                out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", len, len_p));
                let cap_p = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 1\n",
                    cap_p, vec_ptr
                ));
                out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", cap, cap_p));
                let dat_p = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                    dat_p, vec_ptr
                ));
                out.push_str(&format!(
                    "  store ptr {}, ptr {}, align 8\n",
                    data_ptr, dat_p
                ));
                (vec_ptr, Type::Array(Box::new(elem_ty), Some(elems.len())))
            }
            ExprKind::Index { base, index } => {
                let (vec_ptr, base_ty) = self.emit_expr(base, out, locals, None);
                let (idx_val, _) = self.emit_expr(index, out, locals, Some(&Type::I64));
                let elem_ty = match base_ty.strip_region() {
                    Type::Array(e, _) | Type::Vec(e) => (**e).clone(),
                    _ => Type::I64,
                };
                let len_p = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 0\n",
                    len_p, vec_ptr
                ));
                let len_v = self.next_reg();
                out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", len_v, len_p));
                let is_ok = self.next_reg();
                let ok_lbl = self.next_label("bounds_ok");
                let fail_lbl = self.next_label("bounds_fail");
                let file_str = self.intern_string(&expr.span.file);
                out.push_str(&format!("  {} = icmp ult i64 {}, {}\n", is_ok, idx_val, len_v));
                out.push_str(&format!("  br i1 {}, label %{}, label %{}\n", is_ok, ok_lbl, fail_lbl));
                out.push_str(&format!("{}:\n", fail_lbl));
                out.push_str(&format!(
                    "  call void @aps_bounds_check(i64 {}, i64 {}, ptr {}, i64 {})\n  unreachable\n",
                    idx_val, len_v, file_str, expr.span.line
                ));
                out.push_str(&format!("{}:\n", ok_lbl));
                let dat_p = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                    dat_p, vec_ptr
                ));
                let data_ptr = self.next_reg();
                out.push_str(&format!(
                    "  {} = load ptr, ptr {}, align 8\n",
                    data_ptr, dat_p
                ));
                let elem_llvm = Self::llvm_val_type(&elem_ty);
                let ep = self.next_reg();
                out.push_str(&format!(
                    "  {} = getelementptr inbounds {}, ptr {}, i64 {}\n",
                    ep, elem_llvm, data_ptr, idx_val
                ));
                let val = self.next_reg();
                out.push_str(&format!(
                    "  {} = load {}, ptr {}, align 8\n",
                    val, elem_llvm, ep
                ));
                (val, elem_ty)
            }
            ExprKind::Call { callee, args } => self.emit_call(callee, args, out, locals),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.emit_method_call(receiver, method, args, out, locals),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                let (cond_v, _) = self.emit_expr(cond, out, locals, Some(&Type::Bool));
                let cond_i1 = self.next_reg();
                out.push_str(&format!("  {} = icmp ne i64 {}, 0\n", cond_i1, cond_v));
                let then_lbl = self.next_label("if_then");
                let else_lbl = self.next_label("if_else");
                let merge_lbl = self.next_label("if_merge");

                let res_ty = expected_ty.cloned().unwrap_or(Type::I64);
                let res_llvm = Self::llvm_val_type(&res_ty);
                let res_slot = if res_llvm != "void" {
                    self.emit_alloca(res_llvm)
                } else {
                    self.next_reg()
                };

                out.push_str(&format!(
                    "  br i1 {}, label %{}, label %{}\n",
                    cond_i1, then_lbl, else_lbl
                ));

                out.push_str(&format!("{}:\n", then_lbl));
                let mut d1 = Vec::new();
                let mut l1 = Vec::new();
                let (tv, tty, t_ret) = self.emit_block(
                    then_block,
                    out,
                    locals,
                    &mut d1,
                    &mut l1,
                    false,
                    &res_ty,
                );
                if !t_ret {
                    if let Some(v) = tv {
                        if res_llvm != "void" && Self::llvm_val_type(&tty) == res_llvm {
                            out.push_str(&format!(
                                "  store {} {}, ptr {}, align 8\n",
                                res_llvm, v, res_slot
                            ));
                        }
                    }
                    out.push_str(&format!("  br label %{}\n", merge_lbl));
                }

                out.push_str(&format!("{}:\n", else_lbl));
                let mut e_ret = false;
                if let Some(eb) = else_branch {
                    if let ExprKind::Block(eblk) = &eb.kind {
                        let mut d2 = Vec::new();
                        let mut l2 = Vec::new();
                        let (ev, ety, er) = self.emit_block(
                            eblk,
                            out,
                            locals,
                            &mut d2,
                            &mut l2,
                            false,
                            &res_ty,
                        );
                        e_ret = er;
                        if !e_ret {
                            if let Some(v) = ev {
                                if res_llvm != "void" && Self::llvm_val_type(&ety) == res_llvm {
                                    out.push_str(&format!(
                                        "  store {} {}, ptr {}, align 8\n",
                                        res_llvm, v, res_slot
                                    ));
                                }
                            }
                        }
                    } else {
                        let (ev, ety) = self.emit_expr(eb, out, locals, Some(&res_ty));
                        if res_llvm != "void" && Self::llvm_val_type(&ety) == res_llvm {
                            out.push_str(&format!(
                                "  store {} {}, ptr {}, align 8\n",
                                res_llvm, ev, res_slot
                            ));
                        }
                    }
                }
                if !e_ret {
                    out.push_str(&format!("  br label %{}\n", merge_lbl));
                }

                out.push_str(&format!("{}:\n", merge_lbl));
                if res_llvm != "void" {
                    let loaded = self.next_reg();
                    out.push_str(&format!(
                        "  {} = load {}, ptr {}, align 8\n",
                        loaded, res_llvm, res_slot
                    ));
                    (loaded, res_ty)
                } else {
                    ("0".to_string(), Type::Void)
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let (scrut_val, scrut_ty) = self.emit_expr(scrutinee, out, locals, None);
                let merge_lbl = self.next_label("match_end");
                let res_ty = expected_ty.cloned().unwrap_or(Type::I64);
                let res_llvm = Self::llvm_val_type(&res_ty);
                let res_slot = if res_llvm != "void" {
                    let slot = self.emit_alloca(res_llvm);
                    if res_llvm == "ptr" {
                        out.push_str(&format!("  store ptr null, ptr {}, align 8\n", slot));
                    } else if res_llvm == "double" {
                        out.push_str(&format!("  store double 0.0, ptr {}, align 8\n", slot));
                    } else {
                        out.push_str(&format!("  store i64 0, ptr {}, align 8\n", slot));
                    }
                    slot
                } else {
                    self.next_reg()
                };

                let is_enum_ptr = Self::llvm_val_type(&scrut_ty) == "ptr";
                let tag_val = if is_enum_ptr {
                    let tp = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n",
                        tp, scrut_val
                    ));
                    let tv = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", tv, tp));
                    tv
                } else {
                    scrut_val.clone()
                };

                for arm in arms {
                    let arm_lbl = self.next_label("match_arm");
                    let next_lbl = self.next_label("match_next");
                    match &arm.pattern {
                        Pattern::Wildcard | Pattern::Ident(_) => {
                            out.push_str(&format!("  br label %{}\n", arm_lbl));
                        }
                        Pattern::Literal(lit_e) => {
                            let (lv, _) = self.emit_expr(lit_e, out, locals, None);
                            let cmp = self.next_reg();
                            out.push_str(&format!("  {} = icmp eq i64 {}, {}\n", cmp, tag_val, lv));
                            out.push_str(&format!(
                                "  br i1 {}, label %{}, label %{}\n",
                                cmp, arm_lbl, next_lbl
                            ));
                        }
                        Pattern::Variant {
                            enum_name,
                            variant_name,
                            ..
                        } => {
                            let expected_tag = if variant_name == "Some" || variant_name == "Ok" {
                                1i64
                            } else if variant_name == "None" || variant_name == "Err" {
                                0i64
                            } else {
                                let en = enum_name.clone().or_else(|| {
                                    if let Type::Enum(n, _) = &scrut_ty {
                                        Some(n.clone())
                                    } else {
                                        None
                                    }
                                });
                                en.and_then(|n| self.enums.get(&n))
                                    .and_then(|vs| {
                                        vs.iter().position(|(vn, _)| vn == variant_name)
                                    })
                                    .unwrap_or(0) as i64
                            };
                            let cmp = self.next_reg();
                            out.push_str(&format!(
                                "  {} = icmp eq i64 {}, {}\n",
                                cmp, tag_val, expected_tag
                            ));
                            out.push_str(&format!(
                                "  br i1 {}, label %{}, label %{}\n",
                                cmp, arm_lbl, next_lbl
                            ));
                        }
                    }

                    out.push_str(&format!("{}:\n", arm_lbl));
                    if let Pattern::Variant { bindings, .. } = &arm.pattern {
                        if is_enum_ptr {
                            for (bi, bname) in bindings.iter().enumerate() {
                                let field_idx = (bi + 1).min(2);
                                let pp = self.next_reg();
                                out.push_str(&format!(
                                    "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 {}\n",
                                    pp, scrut_val, field_idx
                                ));
                                let pv = self.next_reg();
                                out.push_str(&format!(
                                    "  {} = load i64, ptr {}, align 8\n",
                                    pv, pp
                                ));
                                let bslot = self.emit_alloca("i64");
                                out.push_str(&format!(
                                    "  store i64 {}, ptr {}, align 8\n",
                                    pv, bslot
                                ));
                                locals.insert(
                                    bname.clone(),
                                    LocalVar {
                                        alloca_reg: bslot,
                                        ty: Type::I64,
                                        is_write_ref: false,
                                    },
                                );
                            }
                        }
                    } else if let Pattern::Ident(bname) = &arm.pattern {
                        let slty = Self::llvm_val_type(&scrut_ty);
                        let bslot = self.emit_alloca(slty);
                        out.push_str(&format!(
                            "  store {} {}, ptr {}, align 8\n",
                            slty, scrut_val, bslot
                        ));
                        locals.insert(
                            bname.clone(),
                            LocalVar {
                                alloca_reg: bslot,
                                ty: scrut_ty.clone(),
                                is_write_ref: false,
                            },
                        );
                    }

                    let mut d = Vec::new();
                    let mut l = Vec::new();
                    let (av, aty, aret) =
                        self.emit_block(&arm.body, out, locals, &mut d, &mut l, false, &res_ty);
                    if !aret {
                        if let Some(v) = av {
                            if res_llvm != "void" && Self::llvm_val_type(&aty) == res_llvm {
                                out.push_str(&format!(
                                    "  store {} {}, ptr {}, align 8\n",
                                    res_llvm, v, res_slot
                                ));
                            }
                        }
                        out.push_str(&format!("  br label %{}\n", merge_lbl));
                    }
                    out.push_str(&format!("{}:\n", next_lbl));
                }

                let has_catchall = arms
                    .iter()
                    .any(|a| matches!(a.pattern, Pattern::Wildcard | Pattern::Ident(_)));
                if !has_catchall {
                    let err_str = self.intern_string("non-exhaustive patterns in match expression");
                    let file_str = self.intern_string(&expr.span.file);
                    out.push_str(&format!(
                        "  call void @aps_panic(ptr {}, ptr {}, i64 {})\n  unreachable\n",
                        err_str, file_str, expr.span.line
                    ));
                } else {
                    out.push_str(&format!("  br label %{}\n", merge_lbl));
                }
                out.push_str(&format!("{}:\n", merge_lbl));
                if res_llvm != "void" {
                    let loaded = self.next_reg();
                    out.push_str(&format!(
                        "  {} = load {}, ptr {}, align 8\n",
                        loaded, res_llvm, res_slot
                    ));
                    (loaded, res_ty)
                } else {
                    ("0".to_string(), Type::Void)
                }
            }
            ExprKind::Block(blk) => {
                let mut d = Vec::new();
                let mut l = Vec::new();
                let (v, t, _) =
                    self.emit_block(blk, out, locals, &mut d, &mut l, false, &Type::I64);
                (v.unwrap_or_else(|| "0".to_string()), t)
            }
            ExprKind::TryProp(inner) => {
                let (enum_ptr, inner_ty) = self.emit_expr(inner, out, locals, None);
                let is_ptr = Self::llvm_val_type(&inner_ty) == "ptr";
                let ok_ty = match inner_ty {
                    Type::Result(ok, _) | Type::Option(ok) => *ok,
                    other => other,
                };
                if is_ptr {
                    let pay_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n",
                        pay_p, enum_ptr
                    ));
                    let pay_v = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", pay_v, pay_p));
                    (pay_v, ok_ty)
                } else {
                    (enum_ptr, ok_ty)
                }
            }
            _ => ("0".to_string(), Type::I64),
        }
    }

    fn emit_call(
        &mut self,
        callee: &Expr,
        args: &[CallArg],
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
    ) -> (String, Type) {
        let fn_name = match &callee.kind {
            ExprKind::Ident(n) => n.clone(),
            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(b) = &base.kind {
                    format!("{}::{}", b, field)
                } else {
                    "unknown".to_string()
                }
            }
            _ => "unknown".to_string(),
        };

        // Built-in print / println
        if fn_name == "print" || fn_name == "println" {
            let is_ln = fn_name == "println";
            if args.is_empty() && is_ln {
                let empty = self.intern_string("");
                out.push_str(&format!("  call void @aps_println_str(ptr {})\n", empty));
                return ("0".to_string(), Type::Void);
            }
            for (idx, a) in args.iter().enumerate() {
                let is_last = idx + 1 == args.len();
                let (val, ty) = self.emit_expr(&a.expr, out, locals, None);
                let suffix = if is_ln && is_last { "println" } else { "print" };
                match ty.strip_region() {
                    Type::String | Type::Str => {
                        out.push_str(&format!("  call void @aps_{}_str(ptr {})\n", suffix, val));
                    }
                    Type::F32 | Type::F64 => {
                        out.push_str(&format!(
                            "  call void @aps_{}_f64(double {})\n",
                            suffix, val
                        ));
                    }
                    Type::Bool => {
                        out.push_str(&format!("  call void @aps_{}_bool(i64 {})\n", suffix, val));
                    }
                    _ => {
                        out.push_str(&format!("  call void @aps_{}_i64(i64 {})\n", suffix, val));
                    }
                }
            }
            return ("0".to_string(), Type::Void);
        }

        if fn_name == "eprintln" {
            if let Some(first) = args.first() {
                let (val, _) = self.emit_expr(&first.expr, out, locals, None);
                out.push_str(&format!("  call void @aps_eprintln_str(ptr {})\n", val));
            }
            return ("0".to_string(), Type::Void);
        }

        if fn_name == "panic" {
            let msg_val = if let Some(first) = args.first() {
                self.emit_expr(&first.expr, out, locals, None).0
            } else {
                self.intern_string("explicit panic")
            };
            let file_str = self.intern_string(&callee.span.file);
            out.push_str(&format!(
                "  call void @aps_panic(ptr {}, ptr {}, i64 {})\n",
                msg_val, file_str, callee.span.line
            ));
            return ("0".to_string(), Type::Never);
        }

        if fn_name == "assert" {
            if let Some(first) = args.first() {
                let (cond_v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::Bool));
                let msg = self.intern_string("assertion failed");
                out.push_str(&format!("  call void @aps_assert(i64 {}, ptr {})\n", cond_v, msg));
            }
            return ("0".to_string(), Type::Void);
        }

        if fn_name == "assert_eq" && args.len() >= 2 {
            let (lv, _) = self.emit_expr(&args[0].expr, out, locals, None);
            let (rv, _) = self.emit_expr(&args[1].expr, out, locals, None);
            let cmp = self.next_reg();
            out.push_str(&format!("  {} = icmp eq i64 {}, {}\n", cmp, lv, rv));
            let ext = self.next_reg();
            out.push_str(&format!("  {} = zext i1 {} to i64\n", ext, cmp));
            let msg = self.intern_string("assert_eq failed");
            out.push_str(&format!("  call void @aps_assert(i64 {}, ptr {})\n", ext, msg));
            return ("0".to_string(), Type::Void);
        }

        if fn_name == "share" {
            if let Some(first) = args.first() {
                let (val, ty) = self.emit_expr(&first.expr, out, locals, None);
                return (val, Type::Shared(Box::new(ty)));
            }
        }

        if fn_name == "Some" || fn_name == "Ok" || fn_name == "Err" {
            let tag = if fn_name == "Some" || fn_name == "Ok" {
                1i64
            } else {
                0i64
            };
            let (pay_val, pay_ty) = args
                .first()
                .map(|a| self.emit_expr(&a.expr, out, locals, None))
                .unwrap_or(("0".to_string(), Type::I64));
            let eptr = self.next_reg();
            out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", eptr));
            let tp = self.next_reg();
            out.push_str(&format!(
                "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n",
                tp, eptr
            ));
            out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", tag, tp));
            let pp = self.next_reg();
            out.push_str(&format!(
                "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n",
                pp, eptr
            ));
            if Self::llvm_val_type(&pay_ty) == "i64" {
                out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", pay_val, pp));
            }
            let ret_t = if fn_name == "Some" {
                Type::Option(Box::new(pay_ty))
            } else {
                Type::Result(Box::new(pay_ty), Box::new(Type::String))
            };
            return (eptr, ret_t);
        }

        if fn_name == "time_now_ms" {
            let r = self.next_reg();
            out.push_str(&format!("  {} = call i64 @aps_time_now_ms()\n", r));
            return (r, Type::I64);
        }
        if fn_name == "time_now_ns" {
            let r = self.next_reg();
            out.push_str(&format!("  {} = call i64 @aps_time_now_ns()\n", r));
            return (r, Type::I64);
        }
        if fn_name == "sleep_ms" {
            if let Some(first) = args.first() {
                let (v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::I64));
                out.push_str(&format!("  call void @aps_sleep_ms(i64 {})\n", v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "abs" {
            if let Some(first) = args.first() {
                let (v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::I64));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_abs(i64 {})\n", r, v));
                return (r, Type::I64);
            }
        }
        if fn_name == "to_string" || fn_name == "format" {
            if let Some(first) = args.first() {
                let (v, ty) = self.emit_expr(&first.expr, out, locals, None);
                if matches!(ty, Type::String | Type::Str) {
                    return (v, Type::String);
                }
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_int_to_str(i64 {})\n", r, v));
                return (r, Type::String);
            }
        }

        // Check enum variant constructor `Status::Running(42)`
        if let Some((ename, vname)) = fn_name.split_once("::") {
            if let Some(variants) = self.enums.get(ename).cloned() {
                if let Some(tag) = variants.iter().position(|(vn, _)| vn == vname) {
                    let eptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", eptr));
                    let tp = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n",
                        tp, eptr
                    ));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", tag, tp));
                    if let Some(first) = args.first() {
                        let (pv, pt) = self.emit_expr(&first.expr, out, locals, None);
                        if Self::llvm_val_type(&pt) == "i64" {
                            let pp = self.next_reg();
                            out.push_str(&format!(
                                "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n",
                                pp, eptr
                            ));
                            out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", pv, pp));
                        }
                    }
                    return (eptr, Type::Enum(ename.to_string(), Vec::new()));
                }
            }
        }

        let sanitized_name = Self::sanitize_ident(&fn_name);
        let param_caps = self
            .fn_param_caps
            .get(&sanitized_name)
            .or_else(|| self.fn_param_caps.get(&fn_name))
            .cloned()
            .unwrap_or_default();
        let ret_ty = self
            .fn_ret_types
            .get(&sanitized_name)
            .or_else(|| self.fn_ret_types.get(&fn_name))
            .cloned()
            .unwrap_or(Type::I64);

        let mut llvm_args = Vec::new();
        for (idx, a) in args.iter().enumerate() {
            let (pcap, pty) = param_caps
                .get(idx)
                .cloned()
                .unwrap_or((a.capability, Type::I64));
            let is_write_prim =
                (pcap == Capability::Write || a.capability == Capability::Write) && pty.is_copy();
            if is_write_prim {
                let target_expr = match &a.expr.kind {
                    ExprKind::Unary {
                        op: UnaryOp::Write,
                        expr: inner,
                    } => inner.as_ref(),
                    _ => &a.expr,
                };
                if let Some((ptr_reg, _)) = self.emit_lvalue_ptr(target_expr, out, locals) {
                    llvm_args.push(format!("ptr {}", ptr_reg));
                    continue;
                }
            }
            let (v, actual_ty) = self.emit_expr(&a.expr, out, locals, Some(&pty));
            let alty = Self::llvm_val_type(&actual_ty);
            llvm_args.push(format!("{} {}", alty, v));
        }

        let ret_llvm = Self::llvm_val_type(&ret_ty);
        if ret_llvm == "void" {
            out.push_str(&format!(
                "  call void @{}({})\n",
                sanitized_name,
                llvm_args.join(", ")
            ));
            ("0".to_string(), Type::Void)
        } else {
            let res = self.next_reg();
            out.push_str(&format!(
                "  {} = call {} @{}({})\n",
                res,
                ret_llvm,
                sanitized_name,
                llvm_args.join(", ")
            ));
            (res, ret_ty)
        }
    }

    fn emit_method_call(
        &mut self,
        receiver: &Expr,
        method: &str,
        args: &[CallArg],
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
    ) -> (String, Type) {
        // Static type calls like `Driver.new(...)`, `Status.Running(42)`, `Vec.new()`
        if let ExprKind::Ident(tname) = &receiver.kind {
            if !locals.contains_key(tname) {
                if let Some(variants) = self.enums.get(tname).cloned() {
                    if let Some(tag) = variants.iter().position(|(vn, _)| vn == method) {
                        let eptr = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", eptr));
                        let tp = self.next_reg();
                        out.push_str(&format!(
                            "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n",
                            tp, eptr
                        ));
                        out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", tag, tp));
                        if let Some(first) = args.first() {
                            let (pv, pt) = self.emit_expr(&first.expr, out, locals, None);
                            if Self::llvm_val_type(&pt) == "i64" {
                                let pp = self.next_reg();
                                out.push_str(&format!(
                                    "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n",
                                    pp, eptr
                                ));
                                out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", pv, pp));
                            }
                        }
                        return (eptr, Type::Enum(tname.clone(), Vec::new()));
                    }
                }
                if tname == "Vec" && method == "new" {
                    let vec_ptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", vec_ptr));
                    let data_ptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 128)\n", data_ptr));
                    let cap_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 1\n",
                        cap_p, vec_ptr
                    ));
                    out.push_str(&format!("  store i64 16, ptr {}, align 8\n", cap_p));
                    let dat_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                        dat_p, vec_ptr
                    ));
                    out.push_str(&format!(
                        "  store ptr {}, ptr {}, align 8\n",
                        data_ptr, dat_p
                    ));
                    return (vec_ptr, Type::Vec(Box::new(Type::I64)));
                }
                let full_fn = format!("{}__{}", tname, method);
                let callee = Expr {
                    kind: ExprKind::Ident(full_fn),
                    span: receiver.span.clone(),
                };
                return self.emit_call(&callee, args, out, locals);
            }
        }

        let (recv_val, recv_ty) = self.emit_expr(receiver, out, locals, None);
        match recv_ty.strip_region() {
            Type::RegionArena(rname) if method == "alloc" => {
                if let Some(first) = args.first() {
                    let (val, inner_ty) = self.emit_expr(&first.expr, out, locals, None);
                    // Also invoke `@aps_region_alloc` on the region arena
                    let _ = self.next_reg();
                    out.push_str(&format!(
                        "  call ptr @aps_region_alloc(ptr {}, i64 32)\n",
                        recv_val
                    ));
                    return (
                        val,
                        Type::RegionBound {
                            region: rname.clone(),
                            inner: Box::new(inner_ty),
                        },
                    );
                }
            }
            Type::String | Type::Str => {
                if method == "len" || method == "byte_len" || method == "char_count" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_len(ptr {})\n", r, recv_val));
                    return (r, Type::I64);
                }
                if method == "clone" {
                    return (recv_val, Type::String);
                }
            }
            Type::Array(elem, _) | Type::Vec(elem) => {
                if method == "len" {
                    let lp = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 0\n",
                        lp, recv_val
                    ));
                    let lv = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", lv, lp));
                    return (lv, Type::I64);
                }
                if method == "push" {
                    if let Some(first) = args.first() {
                        let (ev, _) = self.emit_expr(&first.expr, out, locals, Some(elem));
                        out.push_str(&format!(
                            "  call void @aps_vec_push(ptr {}, i64 {})\n",
                            recv_val, ev
                        ));
                        return ("0".to_string(), Type::Void);
                    }
                }
                if method == "pop" {
                    let r = self.next_reg();
                    out.push_str(&format!(
                        "  {} = call i64 @aps_vec_pop(ptr {})\n",
                        r, recv_val
                    ));
                    return (r, elem.as_ref().clone());
                }
            }
            Type::Struct(sname, _) => {
                let full_fn = format!("{}__{}", sname, method);
                let mut full_args = Vec::with_capacity(args.len() + 1);
                full_args.push(CallArg {
                    capability: Capability::Value,
                    expr: receiver.clone(),
                });
                full_args.extend_from_slice(args);
                let callee = Expr {
                    kind: ExprKind::Ident(full_fn),
                    span: receiver.span.clone(),
                };
                return self.emit_call(&callee, &full_args, out, locals);
            }
            _ => {}
        }
        ("0".to_string(), Type::I64)
    }

    pub fn find_clang_executable() -> Option<PathBuf> {
        let candidates = [
            r"C:\Program Files\LLVM\bin\clang.exe",
            r"C:\LLVM\bin\clang.exe",
            "clang",
        ];
        for c in candidates {
            let p = Path::new(c);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }
        if Command::new("clang").arg("--version").output().is_ok() {
            return Some(PathBuf::from("clang"));
        }
        None
    }

    pub fn compile_to_executable(
        &self,
        llvm_ir: &str,
        output_exe: &Path,
        opt_level: u8,
    ) -> Result<(), String> {
        let build_dir = output_exe
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".aps_build");
        fs::create_dir_all(&build_dir)
            .map_err(|e| format!("failed to create build directory: {}", e))?;

        let stem = output_exe
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("module");
        let ll_path = build_dir.join(format!("{}.ll", stem));
        let rt_path = build_dir.join("aps_runtime.c");

        fs::write(&ll_path, llvm_ir)
            .map_err(|e| format!("failed to write LLVM IR file: {}", e))?;
        fs::write(&rt_path, APS_RUNTIME_C_SOURCE)
            .map_err(|e| format!("failed to write APS runtime source: {}", e))?;

        let clang = Self::find_clang_executable().ok_or_else(|| {
            "error[E0702]: LLVM `clang` compiler not found on system".to_string()
        })?;

        let opt_flag = match opt_level {
            0 => "-O0",
            1 => "-O1",
            2 => "-O2",
            _ => "-O3",
        };

        let mut cmd = Command::new(&clang);
        cmd.arg(opt_flag)
            .arg("-Wno-override-module")
            .arg(&ll_path)
            .arg(&rt_path);

        if cfg!(windows) {
            cmd.arg("-lws2_32");
        }

        cmd.arg("-o")
            .arg(output_exe);

        let output = cmd
            .output()
            .map_err(|e| format!("failed to execute LLVM clang: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("error[E0702]: LLVM backend compilation failed:\n{}", stderr));
        }

        Ok(())
    }
}
