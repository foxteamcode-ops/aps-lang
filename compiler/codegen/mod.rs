mod drops;

use crate::compiler::ast::*;
use crate::compiler::hir::HirStruct;
use crate::compiler::ir::{ApsIrFunction, ApsIrModule};
use crate::compiler::types::Type;
use crate::runtime::APS_RUNTIME_C_SOURCE;
use std::collections::{HashMap, HashSet};
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
    current_fn_ret_type: Type,
    current_fn_is_main: bool,
    string_pool: Vec<String>,
    string_map: HashMap<String, (usize, usize)>, // content -> (id, byte_len_with_null)
    structs: HashMap<String, Vec<(String, Type)>>,
    enums: HashMap<String, Vec<(String, Vec<Type>)>>,
    fn_ret_types: HashMap<String, Type>,
    fn_param_caps: HashMap<String, Vec<(Capability, Type)>>,
    loop_stack: Vec<(String, String)>,
    /// `defer_stack.len()` at the moment each active loop was entered (parallel to `loop_stack`).
    loop_defer_depths: Vec<usize>,
    /// Snapshot of the enclosing block's defer stack, inherited by nested if/match/block expressions.
    ambient_defers: Vec<Block>,
    /// Vec locals of the current function proven non-escaping (freed at scope exit).
    droppable_vecs: HashSet<String>,
    reg_counter: usize,
    label_counter: usize,
    pub debug_info: bool,
    di_files: HashMap<String, usize>,
    di_locations: HashMap<(usize, usize, usize), usize>,
    di_meta_nodes: Vec<String>,
    di_meta_counter: usize,
    current_subprogram_node: Option<usize>,
    current_dbg_loc_node: Option<usize>,
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
            current_fn_ret_type: Type::Void,
            current_fn_is_main: false,
            string_pool: Vec::new(),
            string_map: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            fn_ret_types: HashMap::new(),
            fn_param_caps: HashMap::new(),
            loop_stack: Vec::new(),
            loop_defer_depths: Vec::new(),
            ambient_defers: Vec::new(),
            droppable_vecs: HashSet::new(),
            reg_counter: 0,
            label_counter: 0,
            debug_info: false,
            di_files: HashMap::new(),
            di_locations: HashMap::new(),
            di_meta_nodes: Vec::new(),
            di_meta_counter: 6,
            current_subprogram_node: None,
            current_dbg_loc_node: None,
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

    fn get_or_create_di_file(&mut self, path: &str) -> usize {
        let clean_path = if path.is_empty() || path == "<input>" {
            "aps_module.aps"
        } else {
            path
        };
        if let Some(&node) = self.di_files.get(clean_path) {
            return node;
        }
        let p = Path::new(clean_path);
        let filename = p.file_name().and_then(|f| f.to_str()).unwrap_or(clean_path);
        let dir = p.parent().and_then(|d| d.to_str()).unwrap_or(".");
        let dir_escaped = dir.replace('\\', "\\\\");
        let node = self.di_meta_counter;
        self.di_meta_counter += 1;
        self.di_meta_nodes.push(format!(
            "!{} = !DIFile(filename: \"{}\", directory: \"{}\")",
            node, filename, dir_escaped
        ));
        self.di_files.insert(clean_path.to_string(), node);
        node
    }

    fn get_or_create_di_loc(&mut self, line: usize, col: usize, scope: usize) -> usize {
        let key = (line.max(1), col.max(1), scope);
        if let Some(&node) = self.di_locations.get(&key) {
            return node;
        }
        let node = self.di_meta_counter;
        self.di_meta_counter += 1;
        self.di_meta_nodes.push(format!(
            "!{} = !DILocation(line: {}, column: {}, scope: !{})",
            node, key.0, key.1, scope
        ));
        self.di_locations.insert(key, node);
        node
    }

    fn dbg_suffix(&self) -> String {
        if self.debug_info {
            if let Some(loc) = self.current_dbg_loc_node {
                return format!(", !dbg !{}", loc);
            }
        }
        String::new()
    }

    fn emit_alloca(&mut self, llvm_ty: &str) -> String {
        let slot = self.next_reg();
        self.entry_allocas
            .push_str(&format!("  {} = alloca {}, align 8\n", slot, llvm_ty));
        slot
    }

    fn is_struct_all_copy(&self, sname: &str) -> bool {
        let mut visited = HashSet::new();
        self.is_struct_all_copy_visited(sname, &mut visited)
    }

    fn is_struct_all_copy_visited<'a>(&'a self, sname: &'a str, visited: &mut HashSet<&'a str>) -> bool {
        if !visited.insert(sname) {
            return false; // Break cyclic/recursive definition
        }
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
                Type::Struct(nested_name, _) => self.is_struct_all_copy_visited(nested_name, visited),
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
            | Type::Pointer(_)
            | Type::Function(_, _) => "ptr",
            _ => "i64",
        }
    }

    fn unwrap_array_data_ptr(&mut self, reg: String, ty: &Type, out: &mut String) -> String {
        if matches!(ty.strip_region(), Type::Array(_, _) | Type::Vec(_)) {
            let gep = self.next_reg();
            let loaded = self.next_reg();
            out.push_str(&format!("  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n", gep, reg));
            out.push_str(&format!("  {} = load ptr, ptr {}, align 8\n", loaded, gep));
            loaded
        } else {
            reg
        }
    }

    fn emit_store_with_coercion(
        &mut self,
        val: &str,
        from_ty: &str,
        to_ty: &str,
        slot: &str,
        out: &mut String,
    ) {
        if to_ty == "void" {
            return;
        }
        if from_ty == to_ty {
            out.push_str(&format!("  store {} {}, ptr {}, align 8\n", to_ty, val, slot));
        } else if from_ty == "ptr" && to_ty == "i64" {
            let cast = self.next_reg();
            out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", cast, val));
            out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", cast, slot));
        } else if from_ty == "i64" && to_ty == "ptr" {
            let cast = self.next_reg();
            out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", cast, val));
            out.push_str(&format!("  store ptr {}, ptr {}, align 8\n", cast, slot));
        } else if from_ty == "i64" && to_ty == "double" {
            let cast = self.next_reg();
            out.push_str(&format!("  {} = sitofp i64 {} to double\n", cast, val));
            out.push_str(&format!("  store double {}, ptr {}, align 8\n", cast, slot));
        } else if from_ty == "double" && to_ty == "i64" {
            let cast = self.next_reg();
            out.push_str(&format!("  {} = fptosi double {} to i64\n", cast, val));
            out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", cast, slot));
        } else {
            out.push_str(&format!("  store {} {}, ptr {}, align 8\n", to_ty, val, slot));
        }
    }

    fn infer_expr_type(&self, expr: &Expr, locals: &HashMap<String, LocalVar>) -> Option<Type> {
        match &expr.kind {
            ExprKind::IntLit(_, _) => Some(Type::I64),
            ExprKind::FloatLit(_, _) => Some(Type::F64),
            ExprKind::BoolLit(_) => Some(Type::Bool),
            ExprKind::StringLit(_) => Some(Type::String),
            ExprKind::CharLit(_) => Some(Type::Char),
            ExprKind::Ident(name) => locals.get(name).map(|l| l.ty.clone()),
            ExprKind::Call { callee, .. } => {
                if let ExprKind::Ident(fn_name) = &callee.kind {
                    if fn_name == "Some" || fn_name == "None" {
                        return Some(Type::Option(Box::new(Type::Unknown)));
                    }
                    if fn_name == "Ok" || fn_name == "Err" {
                        return Some(Type::Result(Box::new(Type::Unknown), Box::new(Type::Unknown)));
                    }
                    if let Some(ret_ty) = self.fn_ret_types.get(fn_name) {
                        return Some(ret_ty.clone());
                    }
                    if let Some(pos) = fn_name.find('.') {
                        let enum_name = &fn_name[..pos];
                        if self.enums.contains_key(enum_name) {
                            return Some(Type::Enum(enum_name.to_string(), Vec::new()));
                        }
                    }
                }
                None
            }
            ExprKind::StructInit { name, .. } => Some(Type::Struct(name.clone(), Vec::new())),
            ExprKind::Block(b) => self.infer_block_type(b, locals),
            ExprKind::If { then_block, else_branch, .. } => {
                self.infer_block_type(then_block, locals)
                    .or_else(|| else_branch.as_ref().and_then(|eb| self.infer_expr_type(eb, locals)))
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    if let Some(t) = self.infer_block_type(&arm.body, locals) {
                        return Some(t);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn infer_block_type(&self, block: &Block, locals: &HashMap<String, LocalVar>) -> Option<Type> {
        for stmt in block.stmts.iter().rev() {
            match stmt {
                Stmt::Expr { expr, .. } => return self.infer_expr_type(expr, locals),
                Stmt::Return { value: Some(v), .. } => return self.infer_expr_type(v, locals),
                _ => {}
            }
        }
        None
    }

    pub fn generate_llvm_ir(&mut self, module: &ApsIrModule) -> String {
        self.ir_header.clear();
        self.ir_globals.clear();
        self.ir_functions.clear();
        self.di_files.clear();
        self.di_locations.clear();
        self.di_meta_nodes.clear();
        self.di_meta_counter = 7;
        self.current_subprogram_node = None;
        self.current_dbg_loc_node = None;

        if self.debug_info {
            let primary_file = module
                .functions
                .first()
                .map(|f| f.body.span.file.as_str())
                .unwrap_or("aps_module.aps");
            let p = Path::new(if primary_file.is_empty() || primary_file == "<input>" {
                "aps_module.aps"
            } else {
                primary_file
            });
            let filename = p.file_name().and_then(|f| f.to_str()).unwrap_or("aps_module.aps");
            let dir = p.parent().and_then(|d| d.to_str()).unwrap_or(".");
            let dir_escaped = dir.replace('\\', "\\\\");

            self.di_meta_nodes.push("!llvm.dbg.cu = !{!0}".to_string());
            self.di_meta_nodes.push("!llvm.module.flags = !{!2, !3}".to_string());
            self.di_meta_nodes.push("!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !4, producer: \"APS Compiler 1.0\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)".to_string());
            self.di_meta_nodes.push("!2 = !{i32 2, !\"Debug Info Version\", i32 3}".to_string());
            if self.target_triple.contains("windows") {
                self.di_meta_nodes.push("!3 = !{i32 2, !\"CodeView\", i32 1}".to_string());
            } else {
                self.di_meta_nodes.push("!3 = !{i32 2, !\"Dwarf Version\", i32 4}".to_string());
            }
            self.di_meta_nodes.push(format!("!4 = !DIFile(filename: \"{}\", directory: \"{}\")", filename, dir_escaped));
            self.di_files.insert(primary_file.to_string(), 4);
            self.di_meta_nodes.push("!5 = !DISubroutineType(types: !6)".to_string());
            self.di_meta_nodes.push("!6 = !{}".to_string());
        }

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
declare void @aps_vec_free(ptr)
declare void @aps_vec_push(ptr, i64)
declare i64 @aps_vec_pop(ptr)
declare ptr @aps_region_begin(ptr, i64)
declare noalias ptr @aps_region_alloc(ptr, i64) willreturn
declare void @aps_region_end(ptr)
declare void @aps_region_set_quota(ptr, i64)
declare void @aps_region_register_cleanup(ptr, ptr, ptr)
declare i64 @aps_str_len(ptr) readonly willreturn nounwind
declare i64 @aps_str_char_count(ptr) readonly willreturn nounwind
declare i64 @aps_str_starts_with(ptr, ptr) readonly willreturn nounwind
declare i64 @aps_str_ends_with(ptr, ptr) readonly willreturn nounwind
declare i64 @aps_str_eq(ptr, ptr) readonly willreturn nounwind
declare noalias ptr @aps_str_concat(ptr, ptr) willreturn
declare ptr @aps_int_to_str(i64) willreturn
declare ptr @aps_float_to_str(double) willreturn
declare ptr @aps_bool_to_str(i64) willreturn
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
declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i64, i1 immarg)
declare ptr @fs_read_text(ptr)
declare ptr @fs_write_text(ptr, ptr)
declare i64 @fs_exists(ptr)
declare ptr @sha256_hex(ptr)
declare i64 @random_i64(i64, i64)
declare double @aps_sqrt(double)
declare ptr @channel(i64)
declare void @aps_channel_send(ptr, ptr)
declare void @aps_channel_close(ptr)
declare ptr @aps_channel_recv(ptr)
declare ptr @spawn_thread(ptr)
declare i64 @aps_thread_join(ptr)
declare ptr @aps_task_spawn(ptr, ptr)
declare ptr @aps_task_await(ptr)
declare i64 @aps_clock_nanos()
declare double @aps_simd_dot_f64(ptr, ptr, i64)
declare i64 @aps_simd_sum_i64(ptr, i64)
declare i64 @aps_simd_min_i64(ptr, i64)
declare i64 @aps_simd_max_i64(ptr, i64)
declare i64 @aps_simd_find_byte(ptr, i64, i64)
declare i64 @aps_simd_count_byte(ptr, i64, i64)
declare i64 @aps_simd_is_ascii(ptr, i64)
declare void @aps_simd_f64x4_add(ptr, ptr, ptr)
declare void @aps_simd_f64x4_mul(ptr, ptr, ptr)
declare void @aps_simd_i64x4_add(ptr, ptr, ptr)
declare void @aps_simd_i64x4_mul(ptr, ptr, ptr)
declare ptr @aps_region_cast_view(ptr, ptr, i64, i64)
declare ptr @aps_str_slice(ptr, i64, i64)
declare i64 @aps_str_find(ptr, ptr)
declare i64 @aps_str_find_byte(ptr, i64)
declare i64 @aps_str_view_eq(ptr, i64, ptr, i64)
declare ptr @aps_http_response_fast(i64, ptr, ptr)
declare i64 @aps_str_to_i64(ptr) readonly willreturn nounwind
declare i64 @aps_http_server_serve(i64, ptr, i64)
declare ptr @aps_http_client_request(ptr, ptr, i64, ptr, ptr)
declare i64 @llvm.expect.i64(i64, i64)
declare ptr @aps_map_new(i64)
declare void @aps_map_insert(ptr, i64, i64)
declare ptr @aps_map_get(ptr, i64)
declare i64 @aps_map_len(ptr)
declare void @aps_map_free(ptr)
declare ptr @aps_file_create(ptr)
declare ptr @aps_file_open(ptr)
declare ptr @aps_file_write_str(ptr, ptr)
declare ptr @aps_file_flush(ptr)
declare ptr @aps_file_read_to_string(ptr)
declare void @aps_file_close(ptr)
declare ptr @aps_tcp_listener_bind(ptr, i64)
declare ptr @aps_tcp_stream_connect(ptr, i64, i64)
declare ptr @aps_tcp_stream_read_line(i64)
declare ptr @aps_tcp_stream_write_str(i64, ptr)
declare ptr @aps_tcp_stream_flush(i64)
declare ptr @aps_gui_window_create(ptr, i64, i64)
declare void @aps_gui_window_run(ptr)
declare void @aps_gui_window_close(ptr)
declare i64 @aps_gui_add_button(ptr, ptr, i64, i64, i64, i64, ptr)
declare i64 @aps_gui_add_label(ptr, ptr, i64, i64, i64, i64)
declare i64 @aps_gui_add_input(ptr, ptr, i64, i64, i64, i64)
declare void @aps_gui_widget_set_text(ptr, i64, ptr)
declare ptr @aps_gui_widget_get_text(ptr, i64)
declare void @aps_gui_alert(ptr, ptr)
declare i64 @aps_gui_confirm(ptr, ptr)
declare void @aps_gui_error(ptr, ptr)
declare void @aps_gui_window_set_dark_mode(ptr, i64)
declare void @aps_gui_window_set_bg_color(ptr, i64, i64, i64)
declare i64 @aps_gui_add_checkbox(ptr, ptr, i64, i64, i64, i64, i64)
declare i64 @aps_gui_checkbox_is_checked(ptr, i64)
declare void @aps_gui_checkbox_set_checked(ptr, i64, i64)
declare i64 @aps_gui_add_progressbar(ptr, i64, i64, i64, i64)
declare void @aps_gui_progressbar_set_value(ptr, i64, i64)
declare i64 @aps_gui_add_image(ptr, ptr, i64, i64, i64, i64)
declare i64 @aps_gui_add_webview(ptr, ptr, i64, i64, i64, i64)
declare void @aps_gui_webview_navigate(ptr, i64, ptr)
declare void @aps_gui_audio_play(ptr)
declare void @aps_gui_audio_pause()
declare void @aps_gui_audio_resume()
declare void @aps_gui_audio_stop()
declare i64 @aps_gui_video_create(ptr, ptr, i64, i64, i64, i64)
declare void @aps_gui_video_play(ptr, i64)
declare void @aps_gui_video_pause(ptr, i64)
declare void @aps_gui_video_stop(ptr, i64)
declare ptr @aps_gui_file_open(ptr, ptr)
declare ptr @aps_gui_file_save(ptr, ptr)

;Tagged union / Enum standard layout: { i64 tag, i64 payload0, i64 payload1 }
%aps.Enum = type { i64, i64, i64 }
;Dynamic array / Vec layout: { i64 len, i64 cap, ptr data }
%aps.Vec = type { i64, i64, ptr }
;Fat-pointer string layout: { ptr data, i64 len }
%aps.Str = type { ptr, i64 }

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

        // Register and emit extern C functions
        for ext in &module.externs {
            let ret_ty = self.resolve_basic_type_ast(&ext.ret_type).unwrap_or(Type::Void);
            let ret_llvm = Self::llvm_val_type(&ret_ty);
            self.fn_ret_types.insert(ext.name.clone(), ret_ty.clone());
            let mut param_llvms = Vec::new();
            let mut pcaps = Vec::new();
            for p in &ext.params {
                let pty = self.resolve_basic_type_ast(&p.ty).unwrap_or(Type::I64);
                param_llvms.push(Self::llvm_val_type(&pty).to_string());
                pcaps.push((p.capability, pty));
            }
            self.fn_param_caps.insert(ext.name.clone(), pcaps);
            self.ir_header.push_str(&format!(
                "declare {} @{}({})\n",
                ret_llvm,
                ext.name,
                param_llvms.join(", ")
            ));
        }

        // Module-level global constants
        for (cname, cty, _) in &module.consts {
            let lty = Self::llvm_val_type(cty);
            let init_val = if lty == "double" {
                "0.0"
            } else if lty == "ptr" {
                "null"
            } else {
                "0"
            };
            self.ir_globals.push_str(&format!(
                "@const.{} = internal global {} {}, align 8\n",
                cname, lty, init_val
            ));
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
                "@.str.raw.{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n",
                idx, byte_len, escaped
            ));
            self.ir_globals.push_str(&format!(
                "@.str.{} = internal constant %aps.Str {{ ptr @.str.raw.{}, i64 {} }}, align 8\n",
                idx, idx, s.as_bytes().len()
            ));
        }

        let mut di_meta_str = String::new();
        if self.debug_info {
            di_meta_str.push('\n');
            for node in &self.di_meta_nodes {
                di_meta_str.push_str(node);
                di_meta_str.push('\n');
            }
        }

        format!(
            "{}\n{}\n{}\n!1 = !{{!\"branch_weights\", i32 1048576, i32 1}}\n{}",
            self.ir_header, self.ir_globals, self.ir_functions, di_meta_str
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
        self.current_fn_is_main = is_main;
        self.current_fn_ret_type = f.ret_type.clone();
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

        let mut subprog_dbg_attr = String::new();
        if self.debug_info {
            let file_node = self.get_or_create_di_file(&f.body.span.file);
            let line = f.body.span.line.max(1);
            let sp_node = self.di_meta_counter;
            self.di_meta_counter += 1;
            self.di_meta_nodes.push(format!(
                "!{} = distinct !DISubprogram(name: \"{}\", scope: !{}, file: !{}, line: {}, type: !5, scopeLine: {}, spFlags: DISPFlagDefinition, unit: !0)",
                sp_node, f.name, file_node, file_node, line, line
            ));
            self.current_subprogram_node = Some(sp_node);
            let loc_node = self.get_or_create_di_loc(line, f.body.span.col.max(1), sp_node);
            self.current_dbg_loc_node = Some(loc_node);
            subprog_dbg_attr = format!(" !dbg !{}", sp_node);
        } else {
            self.current_subprogram_node = None;
            self.current_dbg_loc_node = None;
        }

        let mut locals: HashMap<String, LocalVar> = HashMap::new();

        // Seed module constants as pointers to the globals
        for (cname, cty, _) in module_consts {
            locals.insert(
                cname.clone(),
                LocalVar {
                    alloca_reg: format!("@const.{}", cname),
                    ty: cty.clone(),
                    is_write_ref: false,
                },
            );
        }

        // Only main initializes module constants once at startup
        if is_main {
            for (cname, cty, cexpr) in module_consts {
                let (cval, actual_ty) = self.emit_expr(cexpr, &mut fn_body, &mut locals, Some(cty));
                let from_ty = Self::llvm_val_type(&actual_ty);
                let to_ty = Self::llvm_val_type(cty);
                self.emit_store_with_coercion(&cval, from_ty, to_ty, &format!("@const.{}", cname), &mut fn_body);
            }
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

        self.loop_stack.clear();
        self.loop_defer_depths.clear();
        let param_names: Vec<String> = f.params.iter().map(|(n, _, _, _)| n.clone()).collect();
        self.droppable_vecs = drops::droppable_vec_locals(&f.body, &param_names);
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
            let dbg_sfx = self.dbg_suffix();
            if is_main {
                fn_body.push_str(&format!("  ret i32 0{}\n", dbg_sfx));
            } else if f.ret_type == Type::Void || f.ret_type == Type::Never {
                fn_body.push_str(&format!("  ret void{}\n", dbg_sfx));
            } else {
                let rty = Self::llvm_val_type(&f.ret_type);
                if let Some(v) = last_val {
                    if Self::llvm_val_type(&last_ty) == rty {
                        fn_body.push_str(&format!("  ret {} {}{}\n", rty, v, dbg_sfx));
                    } else if rty == "ptr" {
                        fn_body.push_str(&format!("  ret ptr null{}\n", dbg_sfx));
                    } else if rty == "double" {
                        fn_body.push_str(&format!("  ret double 0.0{}\n", dbg_sfx));
                    } else {
                        fn_body.push_str(&format!("  ret i64 0{}\n", dbg_sfx));
                    }
                } else if rty == "ptr" {
                    fn_body.push_str(&format!("  ret ptr null{}\n", dbg_sfx));
                } else if rty == "double" {
                    fn_body.push_str(&format!("  ret double 0.0{}\n", dbg_sfx));
                } else {
                    fn_body.push_str(&format!("  ret i64 0{}\n", dbg_sfx));
                }
            }
        }

        let mut out = String::new();
        out.push_str(&format!(
            "define dso_local {} @{}({}){}{} {{\nentry:\n",
            ret_llvm,
            Self::sanitize_ident(&f.name),
            param_sigs.join(", "),
            inline_attr,
            subprog_dbg_attr
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

    fn emit_loop_exit_defers(
        &mut self,
        depth: usize,
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
        defer_stack: &mut Vec<Block>,
        loop_stack: &mut Vec<(String, String)>,
        is_main: bool,
        fn_ret_type: &Type,
    ) {
        let pending: Vec<Block> = defer_stack.iter().skip(depth).cloned().collect();
        for dblk in pending.iter().rev() {
            let mut tmp = Vec::new();
            let _ = self.emit_block(dblk, out, locals, &mut tmp, loop_stack, is_main, fn_ret_type);
        }
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
        let saved_ambient = std::mem::take(&mut self.ambient_defers);
        let r = self.emit_block_inner(block, out, locals, defer_stack, loop_stack, is_main, fn_ret_type);
        self.ambient_defers = saved_ambient;
        r
    }

    fn emit_block_inner(
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
            if self.debug_info {
                if let Some(sp_node) = self.current_subprogram_node {
                    let sp = stmt.span();
                    let loc = self.get_or_create_di_loc(sp.line, sp.col, sp_node);
                    self.current_dbg_loc_node = Some(loc);
                }
            }
            // Nested expression-level blocks (if/match/block) inherit the active defers so that
            // `return`/`break`/`continue` inside them still run enclosing drops.
            self.ambient_defers = defer_stack.clone();
            match stmt {
                Stmt::Let { name, ty, init, .. } | Stmt::Var { name, ty, init, .. } => {
                    let is_stack_struct = if let ExprKind::StructInit { name: sname, .. } = &init.kind {
                        let is_copy = self.is_struct_all_copy(sname);
                        let sfields = self.structs.get(sname).cloned().unwrap_or_default();
                        // Cost-Model: SROA/register promotion only for compact structs (<= 4 fields / <= 32 bytes)
                        let fits_cost_model = sfields.len() <= 4;
                        let escapes = if let Some(ref b) = self.current_fn_body {
                            self.var_escapes_fn(name, b)
                        } else {
                            true
                        };
                        is_copy && fits_cost_model && !escapes
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
                                    let (fval, actual_fty) = self.emit_expr(fexpr, out, locals, Some(fty));
                                    let gep = self.next_reg();
                                    out.push_str(&format!(
                                        "  {} = getelementptr inbounds %struct.{}, ptr {}, i32 0, i32 {}\n",
                                        gep, sname, sptr, idx
                                    ));
                                    let flty = Self::llvm_val_type(fty);
                                    self.emit_store_with_coercion(&fval, Self::llvm_val_type(&actual_fty), flty, &gep, out);
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
                        let final_ty = expected.unwrap_or(inferred_ty.clone());
                        let lty = Self::llvm_val_type(&final_ty);
                        let slot = self.emit_alloca(lty);
                        self.emit_store_with_coercion(&val, Self::llvm_val_type(&inferred_ty), lty, &slot, out);
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
                    let final_ty = expected.unwrap_or(inferred_ty.clone());
                    let lty = Self::llvm_val_type(&final_ty);
                    let slot = self.emit_alloca(lty);
                    self.emit_store_with_coercion(&val, Self::llvm_val_type(&inferred_ty), lty, &slot, out);
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
                        defer_stack.truncate(saved_defer_len);
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
                    let dbg_sfx = self.dbg_suffix();
                    if is_main {
                        out.push_str(&format!("  ret i32 0{}\n", dbg_sfx));
                    } else if let Some((v, ret_ty)) = ret_pair {
                        let rty = Self::llvm_val_type(fn_ret_type);
                        if rty == "void" {
                            out.push_str(&format!("  ret void{}\n", dbg_sfx));
                        } else {
                            let from_ty = Self::llvm_val_type(&ret_ty);
                            if from_ty == rty {
                                out.push_str(&format!("  ret {} {}{}\n", rty, v, dbg_sfx));
                            } else if from_ty == "ptr" && rty == "i64" {
                                let cast = self.next_reg();
                                out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", cast, v));
                                out.push_str(&format!("  ret i64 {}{}\n", cast, dbg_sfx));
                            } else if from_ty == "i64" && rty == "ptr" {
                                let cast = self.next_reg();
                                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", cast, v));
                                out.push_str(&format!("  ret ptr {}{}\n", cast, dbg_sfx));
                            } else if from_ty == "i64" && rty == "double" {
                                let cast = self.next_reg();
                                out.push_str(&format!("  {} = sitofp i64 {} to double\n", cast, v));
                                out.push_str(&format!("  ret double {}{}\n", cast, dbg_sfx));
                            } else if from_ty == "double" && rty == "i64" {
                                let cast = self.next_reg();
                                out.push_str(&format!("  {} = fptosi double {} to i64\n", cast, v));
                                out.push_str(&format!("  ret i64 {}{}\n", cast, dbg_sfx));
                            } else {
                                out.push_str(&format!("  ret {} {}{}\n", rty, v, dbg_sfx));
                            }
                        }
                    } else {
                        let rty = Self::llvm_val_type(&self.current_fn_ret_type);
                        if rty == "void" {
                            out.push_str(&format!("  ret void{}\n", dbg_sfx));
                        } else if rty == "i32" {
                            out.push_str(&format!("  ret i32 0{}\n", dbg_sfx));
                        } else if rty == "i64" {
                            out.push_str(&format!("  ret i64 0{}\n", dbg_sfx));
                        } else if rty == "ptr" {
                            out.push_str(&format!("  ret ptr null{}\n", dbg_sfx));
                        } else if rty == "double" {
                            out.push_str(&format!("  ret double 0.0{}\n", dbg_sfx));
                        } else {
                            out.push_str(&format!("  ret void{}\n", dbg_sfx));
                        }
                    }
                    defer_stack.truncate(saved_defer_len);
                    return (None, Type::Never, true);
                }
                Stmt::Break { .. } => {
                    let bl = self.loop_stack.last().cloned().or_else(|| loop_stack.last().cloned());
                    if let Some((_, break_lbl)) = bl {
                        let depth = self.loop_defer_depths.last().copied().unwrap_or(defer_stack.len());
                        self.emit_loop_exit_defers(depth, out, locals, defer_stack, loop_stack, is_main, fn_ret_type);
                        out.push_str(&format!("  br label %{}\n", break_lbl));
                        defer_stack.truncate(saved_defer_len);
                        return (None, Type::Never, true);
                    }
                }
                Stmt::Continue { .. } => {
                    let cl = self.loop_stack.last().cloned().or_else(|| loop_stack.last().cloned());
                    if let Some((cond_lbl, _)) = cl {
                        let depth = self.loop_defer_depths.last().copied().unwrap_or(defer_stack.len());
                        self.emit_loop_exit_defers(depth, out, locals, defer_stack, loop_stack, is_main, fn_ret_type);
                        out.push_str(&format!("  br label %{}\n", cond_lbl));
                        defer_stack.truncate(saved_defer_len);
                        return (None, Type::Never, true);
                    }
                }
                Stmt::While { cond, body, .. } => {
                    let cond_lbl = self.next_label("while_cond");
                    let body_lbl = self.next_label("while_body");
                    let end_lbl = self.next_label("while_end");

                    out.push_str(&format!("  br label %{}\n", cond_lbl));
                    out.push_str(&format!("{}:\n", cond_lbl));
                    let cond_i1 = self.emit_cond(cond, out, locals);
                    out.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n",
                        cond_i1, body_lbl, end_lbl
                    ));

                    out.push_str(&format!("{}:\n", body_lbl));
                    loop_stack.push((cond_lbl.clone(), end_lbl.clone()));
                    self.loop_stack.push((cond_lbl.clone(), end_lbl.clone()));
                    self.loop_defer_depths.push(defer_stack.len());
                    let (_, _, body_terminated) = self.emit_block(
                        body,
                        out,
                        locals,
                        defer_stack,
                        loop_stack,
                        is_main,
                        fn_ret_type,
                    );
                    self.loop_stack.pop();
                    self.loop_defer_depths.pop();
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
                    self.loop_stack.push((body_lbl.clone(), end_lbl.clone()));
                    self.loop_defer_depths.push(defer_stack.len());
                    let (_, _, body_terminated) = self.emit_block(
                        body,
                        out,
                        locals,
                        defer_stack,
                        loop_stack,
                        is_main,
                        fn_ret_type,
                    );
                    self.loop_stack.pop();
                    self.loop_defer_depths.pop();
                    loop_stack.pop();
                    if !body_terminated {
                        out.push_str(&format!("  br label %{}\n", body_lbl));
                    }
                    out.push_str(&format!("{}:\n", end_lbl));
                }
                Stmt::For {
                    var, iter, body, ..
                } => {
                    if let ExprKind::Range { start, end, inclusive } = &iter.kind {
                        let (start_v, _) = self.emit_expr(start, out, locals, Some(&Type::I64));
                        let (end_v, _) = self.emit_expr(end, out, locals, Some(&Type::I64));
                        let var_slot = self.emit_alloca("i64");
                        out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", start_v, var_slot));
                        locals.insert(
                            var.clone(),
                            LocalVar {
                                alloca_reg: var_slot.clone(),
                                ty: Type::I64,
                                is_write_ref: false,
                            },
                        );
                        let cond_lbl = self.next_label("for_cond");
                        let body_lbl = self.next_label("for_body");
                        let step_lbl = self.next_label("for_step");
                        let end_lbl = self.next_label("for_end");

                        out.push_str(&format!("  br label %{}\n", cond_lbl));
                        out.push_str(&format!("{}:\n", cond_lbl));
                        let cur = self.next_reg();
                        out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", cur, var_slot));
                        let cmp = self.next_reg();
                        let op = if *inclusive { "sle" } else { "slt" };
                        out.push_str(&format!("  {} = icmp {} i64 {}, {}\n", cmp, op, cur, end_v));
                        out.push_str(&format!("  br i1 {}, label %{}, label %{}\n", cmp, body_lbl, end_lbl));

                        out.push_str(&format!("{}:\n", body_lbl));
                        loop_stack.push((step_lbl.clone(), end_lbl.clone()));
                        self.loop_stack.push((step_lbl.clone(), end_lbl.clone()));
                        self.loop_defer_depths.push(defer_stack.len());
                        let (_, _, term) = self.emit_block(
                            body,
                            out,
                            locals,
                            defer_stack,
                            loop_stack,
                            is_main,
                            fn_ret_type,
                        );
                        self.loop_stack.pop();
                        self.loop_defer_depths.pop();
                        loop_stack.pop();
                        if !term {
                            out.push_str(&format!("  br label %{}\n", step_lbl));
                        }
                        out.push_str(&format!("{}:\n", step_lbl));
                        let c_now = self.next_reg();
                        out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", c_now, var_slot));
                        let c_next = self.next_reg();
                        out.push_str(&format!("  {} = add i64 {}, 1\n", c_next, c_now));
                        out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", c_next, var_slot));
                        out.push_str(&format!("  br label %{}\n", cond_lbl));
                        out.push_str(&format!("{}:\n", end_lbl));
                    } else {
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
                        self.loop_stack.push((step_lbl.clone(), end_lbl.clone()));
                        self.loop_defer_depths.push(defer_stack.len());
                        let (_, _, term) = self.emit_block(
                            body,
                            out,
                            locals,
                            defer_stack,
                            loop_stack,
                            is_main,
                            fn_ret_type,
                        );
                        self.loop_stack.pop();
                        self.loop_defer_depths.pop();
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
                }
                Stmt::Expr { expr, .. } => {
                    let (v, t) = self.emit_expr(expr, out, locals, None);
                    if is_last {
                        last_val = Some(v);
                        last_ty = t;
                    }
                }
            }

            // Deterministic drop: schedule `aps_vec_free` for proven non-escaping Vec locals.
            if let Stmt::Let { name, init, .. } | Stmt::Var { name, init, .. } = stmt {
                if self.droppable_vecs.contains(name) {
                    let sp = init.span.clone();
                    defer_stack.push(Block {
                        stmts: vec![Stmt::Expr {
                            expr: Expr {
                                kind: ExprKind::Call {
                                    callee: Box::new(Expr {
                                        kind: ExprKind::Ident("__aps_drop_vec".to_string()),
                                        span: sp.clone(),
                                    }),
                                    args: vec![CallArg {
                                        capability: Capability::Read,
                                        expr: Expr {
                                            kind: ExprKind::Ident(name.clone()),
                                            span: sp.clone(),
                                        },
                                    }],
                                },
                                span: sp.clone(),
                            },
                            has_semi: true,
                            span: sp.clone(),
                        }],
                        span: sp,
                    });
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
                out.push_str(&format!("  br i1 {}, label %{}, label %{}, !prof !1\n", is_ok, ok_lbl, fail_lbl));
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
                "HashMap" => {
                    let k = args
                        .get(0)
                        .and_then(|a| self.resolve_basic_type_ast(a))
                        .unwrap_or(Type::String);
                    let v = args
                        .get(1)
                        .and_then(|a| self.resolve_basic_type_ast(a))
                        .unwrap_or(Type::I64);
                    Some(Type::HashMap(Box::new(k), Box::new(v)))
                }
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
                "Channel" | "Receiver" | "Sender" => {
                    let inner = args
                        .first()
                        .and_then(|a| self.resolve_basic_type_ast(a))
                        .unwrap_or(Type::I64);
                    Some(Type::Channel(Box::new(inner)))
                }
                "Task" => {
                    let inner = args
                        .first()
                        .and_then(|a| self.resolve_basic_type_ast(a))
                        .unwrap_or(Type::I64);
                    Some(Type::Task(Box::new(inner)))
                }
                other => {
                    if self.enums.contains_key(other) {
                        Some(Type::Enum(other.to_string(), Vec::new()))
                    } else if self.structs.contains_key(other) {
                        Some(Type::Struct(other.to_string(), Vec::new()))
                    } else {
                        Some(Type::GenericParam(other.to_string()))
                    }
                }
            },
            _ => None,
        }
    }

    fn emit_cond(
        &mut self,
        cond: &Expr,
        out: &mut String,
        locals: &mut HashMap<String, LocalVar>,
    ) -> String {
        match &cond.kind {
            ExprKind::Binary { op, left, right } => match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    let (lv, lt) = self.emit_expr(left, out, locals, None);
                    let (rv, _) = self.emit_expr(right, out, locals, Some(&lt));
                    let is_float = matches!(lt.strip_region(), Type::F64 | Type::F32);
                    let is_str = matches!(lt.strip_region(), Type::String | Type::Str);
                    if is_str {
                        let eq = self.next_reg();
                        out.push_str(&format!(
                            "  {} = call i64 @aps_str_eq(ptr {}, ptr {})\n",
                            eq, lv, rv
                        ));
                        let cmp_i1 = self.next_reg();
                        let cmp_kw = if *op == BinOp::Ne { "eq" } else { "ne" };
                        out.push_str(&format!("  {} = icmp {} i64 {}, 0\n", cmp_i1, cmp_kw, eq));
                        cmp_i1
                    } else if is_float {
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
                        cmp_i1
                    } else {
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
                        cmp_i1
                    }
                }
                BinOp::And => {
                    let c1 = self.emit_cond(left, out, locals);
                    let c2 = self.emit_cond(right, out, locals);
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = and i1 {}, {}\n", r, c1, c2));
                    r
                }
                BinOp::Or => {
                    let c1 = self.emit_cond(left, out, locals);
                    let c2 = self.emit_cond(right, out, locals);
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = or i1 {}, {}\n", r, c1, c2));
                    r
                }
                _ => {
                    let (cond_v, _) = self.emit_expr(cond, out, locals, Some(&Type::Bool));
                    let cond_i1 = self.next_reg();
                    out.push_str(&format!("  {} = icmp ne i64 {}, 0\n", cond_i1, cond_v));
                    cond_i1
                }
            },
            ExprKind::Unary {
                op: UnaryOp::Not,
                expr: inner,
            } => {
                let c = self.emit_cond(inner, out, locals);
                let r = self.next_reg();
                out.push_str(&format!("  {} = xor i1 {}, true\n", r, c));
                r
            }
            ExprKind::BoolLit(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            _ => {
                let (cond_v, _) = self.emit_expr(cond, out, locals, Some(&Type::Bool));
                let cond_i1 = self.next_reg();
                out.push_str(&format!("  {} = icmp ne i64 {}, 0\n", cond_i1, cond_v));
                cond_i1
            }
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
                if self.fn_ret_types.contains_key(name) {
                    let ret_ty = self.fn_ret_types.get(name).cloned().unwrap_or(Type::Void);
                    let pcaps = self.fn_param_caps.get(name).cloned().unwrap_or_default();
                    return (format!("@{}", name), Type::Function(pcaps, Box::new(ret_ty)));
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
                UnaryOp::Share => {
                    let (v, inner_ty) = self.emit_expr(inner, out, locals, expected_ty);
                    (v, Type::Shared(Box::new(inner_ty)))
                }
                UnaryOp::Read | UnaryOp::Write | UnaryOp::Take => self.emit_expr(inner, out, locals, expected_ty),
                UnaryOp::Spawn => {
                    let maybe_fn_name = match &inner.kind {
                        ExprKind::Call { callee, .. } => match &callee.kind {
                            ExprKind::Ident(n) => Some(n.clone()),
                            ExprKind::Path(segments) => Some(segments.join("__")),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let (Some(fn_name), ExprKind::Call { args, .. }) = (maybe_fn_name, &inner.kind) {
                        let ret_ty = self.fn_ret_types.get(&fn_name).cloned().unwrap_or(Type::I64);
                        let param_caps = self.fn_param_caps.get(&fn_name).cloned().unwrap_or_default();
                        let mut eval_args = Vec::new();
                        for (idx, a) in args.iter().enumerate() {
                            let exp_p = param_caps.get(idx).map(|(_, t)| t);
                            let (arg_v, arg_t) = self.emit_expr(&a.expr, out, locals, exp_p);
                            eval_args.push((arg_v, arg_t));
                        }
                        let tramp_name = self.next_label("__spawn_tramp");
                        let arg_count = eval_args.len();
                        let arg_buf = if arg_count > 0 {
                            let struct_size = (arg_count * 8) as i64;
                            let buf_reg = self.next_reg();
                            out.push_str(&format!("  {} = call ptr @aps_alloc(i64 {})\n", buf_reg, struct_size));
                            for (idx, (arg_v, arg_t)) in eval_args.iter().enumerate() {
                                let gep = self.next_reg();
                                let llvm_t = Self::llvm_val_type(arg_t);
                                out.push_str(&format!("  {} = getelementptr inbounds {}, ptr {}, i64 {}\n", gep, llvm_t, buf_reg, idx));
                                out.push_str(&format!("  store {} {}, ptr {}, align 8\n", llvm_t, arg_v, gep));
                            }
                            buf_reg
                        } else {
                            "null".to_string()
                        };

                        let mut tramp_ir = format!("define private ptr @{}(ptr %arg_buf) {{\nentry:\n", tramp_name);
                        let mut call_args = Vec::new();
                        for (idx, (_, arg_t)) in eval_args.iter().enumerate() {
                            let gep = format!("%arg_{}", idx);
                            let val = format!("%val_{}", idx);
                            let llvm_t = Self::llvm_val_type(arg_t);
                            tramp_ir.push_str(&format!("  {} = getelementptr inbounds {}, ptr %arg_buf, i64 {}\n", gep, llvm_t, idx));
                            tramp_ir.push_str(&format!("  {} = load {}, ptr {}, align 8\n", val, llvm_t, gep));
                            call_args.push(format!("{} {}", llvm_t, val));
                        }
                        let ret_llvm = Self::llvm_val_type(&ret_ty);
                        if ret_llvm == "void" {
                            tramp_ir.push_str(&format!("  call void @{}({})\n", fn_name, call_args.join(", ")));
                            tramp_ir.push_str("  ret ptr null\n}\n");
                        } else if ret_llvm == "ptr" {
                            tramp_ir.push_str(&format!("  %ret_val = call ptr @{}({})\n", fn_name, call_args.join(", ")));
                            tramp_ir.push_str("  ret ptr %ret_val\n}\n");
                        } else if ret_llvm == "double" {
                            tramp_ir.push_str(&format!("  %ret_val = call double @{}({})\n", fn_name, call_args.join(", ")));
                            tramp_ir.push_str("  %cast_i64 = bitcast double %ret_val to i64\n");
                            tramp_ir.push_str("  %res_ptr = inttoptr i64 %cast_i64 to ptr\n");
                            tramp_ir.push_str("  ret ptr %res_ptr\n}\n");
                        } else {
                            tramp_ir.push_str(&format!("  %ret_val = call i64 @{}({})\n", fn_name, call_args.join(", ")));
                            tramp_ir.push_str("  %res_ptr = inttoptr i64 %ret_val to ptr\n");
                            tramp_ir.push_str("  ret ptr %res_ptr\n}\n");
                        }
                        self.ir_functions.push_str(&tramp_ir);

                        let task_reg = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_task_spawn(ptr @{}, ptr {})\n", task_reg, tramp_name, arg_buf));
                        (task_reg, Type::Task(Box::new(ret_ty)))
                    } else {
                        let (v, inner_ty) = self.emit_expr(inner, out, locals, expected_ty);
                        (v, Type::Task(Box::new(inner_ty)))
                    }
                }
                UnaryOp::Await => {
                    let (v, inner_ty) = self.emit_expr(inner, out, locals, expected_ty);
                    match inner_ty {
                        Type::Task(target_ty) => {
                            let raw_res = self.next_reg();
                            out.push_str(&format!("  {} = call ptr @aps_task_await(ptr {})\n", raw_res, v));
                            let target_llvm = Self::llvm_val_type(&target_ty);
                            if target_llvm == "void" {
                                ("0".to_string(), Type::Void)
                            } else if target_llvm == "ptr" {
                                (raw_res, *target_ty)
                            } else if target_llvm == "double" {
                                let as_int = self.next_reg();
                                out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", as_int, raw_res));
                                let as_dbl = self.next_reg();
                                out.push_str(&format!("  {} = bitcast i64 {} to double\n", as_dbl, as_int));
                                (as_dbl, *target_ty)
                            } else {
                                let as_int = self.next_reg();
                                out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", as_int, raw_res));
                                (as_int, *target_ty)
                            }
                        }
                        other => (v, other),
                    }
                }
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
                            let mut actual_lv = lv;
                            if !matches!(lty, Type::String | Type::Str) {
                                let conv = self.next_reg();
                                if matches!(lty, Type::F32 | Type::F64) {
                                    out.push_str(&format!("  {} = call ptr @aps_float_to_str(double {})\n", conv, actual_lv));
                                } else if lty == Type::Bool {
                                    out.push_str(&format!("  {} = call ptr @aps_bool_to_str(i64 {})\n", conv, actual_lv));
                                } else {
                                    out.push_str(&format!("  {} = call ptr @aps_int_to_str(i64 {})\n", conv, actual_lv));
                                }
                                actual_lv = conv;
                            }
                            let mut actual_rv = rv;
                            if !matches!(rty, Type::String | Type::Str) {
                                let conv = self.next_reg();
                                if matches!(rty, Type::F32 | Type::F64) {
                                    out.push_str(&format!("  {} = call ptr @aps_float_to_str(double {})\n", conv, actual_rv));
                                } else if rty == Type::Bool {
                                    out.push_str(&format!("  {} = call ptr @aps_bool_to_str(i64 {})\n", conv, actual_rv));
                                } else {
                                    out.push_str(&format!("  {} = call ptr @aps_int_to_str(i64 {})\n", conv, actual_rv));
                                }
                                actual_rv = conv;
                            }
                            out.push_str(&format!(
                                "  {} = call ptr @aps_str_concat(ptr {}, ptr {})\n",
                                res, actual_lv, actual_rv
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
                                let is_non_zero = self.next_reg();
                                let div_ok = self.next_label("div_ok");
                                let div_fail = self.next_label("div_fail");
                                let err_str = self.intern_string("attempt to divide by zero");
                                let file_str = self.intern_string(&expr.span.file);
                                out.push_str(&format!("  {} = icmp ne i64 {}, 0\n", is_non_zero, rv));
                                out.push_str(&format!("  br i1 {}, label %{}, label %{}, !prof !1\n", is_non_zero, div_ok, div_fail));
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
                                let is_non_zero = self.next_reg();
                                let rem_ok = self.next_label("rem_ok");
                                let rem_fail = self.next_label("rem_fail");
                                let err_str = self.intern_string("attempt to calculate remainder with zero divisor");
                                let file_str = self.intern_string(&expr.span.file);
                                out.push_str(&format!("  {} = icmp ne i64 {}, 0\n", is_non_zero, rv));
                                out.push_str(&format!("  br i1 {}, label %{}, label %{}, !prof !1\n", is_non_zero, rem_ok, rem_fail));
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
                        let (fval, actual_fty) = self.emit_expr(fexpr, out, locals, Some(fty));
                        let gep = self.next_reg();
                        out.push_str(&format!(
                            "  {} = getelementptr inbounds %struct.{}, ptr {}, i32 0, i32 {}\n",
                            gep, name, sptr, idx
                        ));
                        let flty = Self::llvm_val_type(fty);
                        self.emit_store_with_coercion(&fval, Self::llvm_val_type(&actual_fty), flty, &gep, out);
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
                if let Type::Channel(inner) = clean_ty {
                    if field == "sender" || field == "receiver" {
                        return (base_ptr, Type::Channel(inner.clone()));
                    }
                }
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
                out.push_str(&format!("  br i1 {}, label %{}, label %{}, !prof !1\n", is_ok, ok_lbl, fail_lbl));
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
                let cond_i1 = self.emit_cond(cond, out, locals);
                let then_lbl = self.next_label("if_then");
                let else_lbl = self.next_label("if_else");
                let merge_lbl = self.next_label("if_merge");

                let res_ty = expected_ty.cloned()
                    .or_else(|| self.infer_block_type(then_block, locals))
                    .or_else(|| else_branch.as_ref().and_then(|eb| self.infer_expr_type(eb, locals)))
                    .unwrap_or(Type::Void);
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

                out.push_str(&format!(
                    "  br i1 {}, label %{}, label %{}\n",
                    cond_i1, then_lbl, else_lbl
                ));

                let cur_ret_ty = self.current_fn_ret_type.clone();
                let is_main = self.current_fn_is_main;
                out.push_str(&format!("{}:\n", then_lbl));
                let mut d1 = self.ambient_defers.clone();
                let mut l1 = Vec::new();
                let (tv, tty, t_ret) = self.emit_block(
                    then_block,
                    out,
                    locals,
                    &mut d1,
                    &mut l1,
                    is_main,
                    &cur_ret_ty,
                );
                if !t_ret {
                    if let Some(v) = tv {
                        if res_llvm != "void" {
                            self.emit_store_with_coercion(&v, Self::llvm_val_type(&tty), res_llvm, &res_slot, out);
                        }
                    }
                    out.push_str(&format!("  br label %{}\n", merge_lbl));
                }

                out.push_str(&format!("{}:\n", else_lbl));
                let mut e_ret = false;
                if let Some(eb) = else_branch {
                    if let ExprKind::Block(eblk) = &eb.kind {
                        let mut d2 = self.ambient_defers.clone();
                        let mut l2 = Vec::new();
                        let (ev, ety, er) = self.emit_block(
                            eblk,
                            out,
                            locals,
                            &mut d2,
                            &mut l2,
                            is_main,
                            &cur_ret_ty,
                        );
                        e_ret = er;
                        if !e_ret {
                            if let Some(v) = ev {
                                if res_llvm != "void" {
                                    self.emit_store_with_coercion(&v, Self::llvm_val_type(&ety), res_llvm, &res_slot, out);
                                }
                            }
                        }
                    } else {
                        let (ev, ety) = self.emit_expr(eb, out, locals, Some(&res_ty));
                        if res_llvm != "void" {
                            self.emit_store_with_coercion(&ev, Self::llvm_val_type(&ety), res_llvm, &res_slot, out);
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
                let res_ty = expected_ty.cloned()
                    .or_else(|| {
                        for arm in arms {
                            if let Some(t) = self.infer_block_type(&arm.body, locals) {
                                return Some(t);
                            }
                        }
                        None
                    })
                    .unwrap_or(Type::I64);
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
                            let clean_scrut = scrut_ty.strip_region();
                            let expected_tag = if enum_name.is_none() && matches!(clean_scrut, Type::Option(_)) {
                                if variant_name == "Some" { 1i64 } else { 0i64 }
                            } else if enum_name.is_none() && matches!(clean_scrut, Type::Result(_, _)) {
                                if variant_name == "Ok" { 1i64 } else { 0i64 }
                            } else {
                                let en = enum_name.clone().or_else(|| {
                                    if let Type::Enum(n, _) = clean_scrut {
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
                    if let Pattern::Variant {
                        variant_name,
                        bindings,
                        ..
                    } = &arm.pattern
                    {
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
                                let bound_ty = match scrut_ty.strip_region() {
                                    Type::Option(inner) => (**inner).clone(),
                                    Type::Result(ok, err) => {
                                        if variant_name == "Ok" {
                                            (**ok).clone()
                                        } else {
                                            (**err).clone()
                                        }
                                    }
                                    Type::Enum(ename, _) => self
                                        .enums
                                        .get(ename)
                                        .and_then(|vs| {
                                            vs.iter().find(|(vn, _)| vn == variant_name)
                                        })
                                        .and_then(|(_, tys)| tys.get(bi).cloned())
                                        .unwrap_or(Type::I64),
                                    _ => Type::I64,
                                };
                                let bound_llvm = Self::llvm_val_type(&bound_ty);
                                let bslot = self.emit_alloca(bound_llvm);
                                if bound_llvm == "ptr" {
                                    let conv = self.next_reg();
                                    out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", conv, pv));
                                    out.push_str(&format!("  store ptr {}, ptr {}, align 8\n", conv, bslot));
                                } else if bound_llvm == "double" {
                                    let conv = self.next_reg();
                                    out.push_str(&format!("  {} = bitcast i64 {} to double\n", conv, pv));
                                    out.push_str(&format!("  store double {}, ptr {}, align 8\n", conv, bslot));
                                } else {
                                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", pv, bslot));
                                }
                                locals.insert(
                                    bname.clone(),
                                    LocalVar {
                                        alloca_reg: bslot,
                                        ty: bound_ty,
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

                    let mut d = self.ambient_defers.clone();
                    let mut l = self.loop_stack.clone();
                    let cur_ret_ty = self.current_fn_ret_type.clone();
                    let is_main = self.current_fn_is_main;
                    let (av, aty, aret) =
                        self.emit_block(&arm.body, out, locals, &mut d, &mut l, is_main, &cur_ret_ty);
                    if !aret {
                        if let Some(v) = av {
                            self.emit_store_with_coercion(&v, Self::llvm_val_type(&aty), res_llvm, &res_slot, out);
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
                let mut d = self.ambient_defers.clone();
                let mut l = Vec::new();
                let cur_ret_ty = self.current_fn_ret_type.clone();
                let is_main = self.current_fn_is_main;
                let (v, t, _) =
                    self.emit_block(blk, out, locals, &mut d, &mut l, is_main, &cur_ret_ty);
                (v.unwrap_or_else(|| "0".to_string()), t)
            }
            ExprKind::TryProp(inner) => {
                let (enum_ptr, inner_ty) = self.emit_expr(inner, out, locals, None);
                let is_ptr = Self::llvm_val_type(&inner_ty) == "ptr";
                let ok_ty = match inner_ty.clone() {
                    Type::Result(ok, _) | Type::Option(ok) => *ok,
                    other => other,
                };
                if is_ptr {
                    let tag_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n",
                        tag_p, enum_ptr
                    ));
                    let tag_v = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", tag_v, tag_p));
                    let is_ok = self.next_reg();
                    let ok_lbl = self.next_label("try_ok");
                    let err_lbl = self.next_label("try_err");
                    out.push_str(&format!("  {} = icmp eq i64 {}, 1\n", is_ok, tag_v));
                    out.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}, !prof !1\n",
                        is_ok, ok_lbl, err_lbl
                    ));
                    out.push_str(&format!("{}:\n", err_lbl));
                    let cur_ret_llvm = Self::llvm_val_type(&self.current_fn_ret_type);
                    if cur_ret_llvm == "ptr" {
                        out.push_str(&format!("  ret ptr {}\n", enum_ptr));
                    } else if cur_ret_llvm == "void" {
                        out.push_str("  ret void\n");
                    } else {
                        out.push_str(&format!("  ret {} 0\n", cur_ret_llvm));
                    }
                    out.push_str(&format!("{}:\n", ok_lbl));
                    let pay_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n",
                        pay_p, enum_ptr
                    ));
                    let pay_v = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", pay_v, pay_p));
                    let ok_llvm = Self::llvm_val_type(&ok_ty);
                    let final_val = if ok_llvm == "ptr" {
                        let conv = self.next_reg();
                        out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", conv, pay_v));
                        conv
                    } else if ok_llvm == "double" {
                        let conv = self.next_reg();
                        out.push_str(&format!("  {} = bitcast i64 {} to double\n", conv, pay_v));
                        conv
                    } else {
                        pay_v
                    };
                    (final_val, ok_ty)
                } else {
                    (enum_ptr, ok_ty)
                }
            }
            ExprKind::Closure {
                params,
                ret_type,
                body,
            } => {
                let cid = self.reg_counter;
                self.reg_counter += 1;
                let c_fn_name = format!("__aps_closure_{}", cid);

                let mut param_types = Vec::new();
                let mut param_sigs = Vec::new();
                let mut closure_locals = HashMap::new();
                let mut c_entry_allocas = String::new();

                for p in params {
                    let pt = match &p.ty {
                        TypeAst::Named { name, .. } if name == "f64" || name == "f32" => Type::F64,
                        TypeAst::Named { name, .. } if name == "bool" => Type::Bool,
                        TypeAst::Named { name, .. } if name == "String" || name == "str" => Type::String,
                        _ => Type::I64,
                    };
                    let pllvm = Self::llvm_val_type(&pt);
                    let arg_name = format!("%arg.{}", p.name);
                    param_sigs.push(format!("{} {}", pllvm, arg_name));
                    param_types.push((p.capability, pt.clone()));
                    let slot = self.next_reg();
                    c_entry_allocas.push_str(&format!("  {} = alloca {}, align 8\n", slot, pllvm));
                    c_entry_allocas.push_str(&format!(
                        "  store {} {}, ptr {}, align 8\n",
                        pllvm, arg_name, slot
                    ));
                    closure_locals.insert(
                        p.name.clone(),
                        LocalVar {
                            alloca_reg: slot,
                            ty: pt,
                            is_write_ref: false,
                        },
                    );
                }

                let ret_ty = match ret_type {
                    TypeAst::Named { name, .. } if name == "f64" || name == "f32" => Type::F64,
                    TypeAst::Named { name, .. } if name == "bool" => Type::Bool,
                    TypeAst::Named { name, .. } if name == "void" => Type::Void,
                    TypeAst::Named { name, .. } if name == "String" || name == "str" => Type::String,
                    _ => Type::I64,
                };
                let ret_llvm = Self::llvm_val_type(&ret_ty);

                for (var_name, var_info) in locals.iter() {
                    if !closure_locals.contains_key(var_name) && Self::block_has_bare_ident(body, var_name) {
                        let env_global = format!("@__env_{}", var_name);
                        let var_llvm = Self::llvm_val_type(&var_info.ty);
                        let default_val = if var_llvm == "ptr" {
                            "null"
                        } else if var_llvm == "double" {
                            "0.0"
                        } else {
                            "0"
                        };
                        if !self.ir_globals.contains(&format!("{} =", env_global)) {
                            self.ir_globals.push_str(&format!(
                                "{} = internal global {} {}, align 8\n",
                                env_global, var_llvm, default_val
                            ));
                        }
                        let cur_val = self.next_reg();
                        out.push_str(&format!(
                            "  {} = load {}, ptr {}, align 8\n",
                            cur_val, var_llvm, var_info.alloca_reg
                        ));
                        out.push_str(&format!(
                            "  store {} {}, ptr {}, align 8\n",
                            var_llvm, cur_val, env_global
                        ));

                        closure_locals.insert(
                            var_name.clone(),
                            LocalVar {
                                alloca_reg: env_global,
                                ty: var_info.ty.clone(),
                                is_write_ref: false,
                            },
                        );
                    }
                }

                let mut c_body = String::new();
                let mut c_defers = Vec::new();
                let mut c_locals_track = Vec::new();
                let (last_val, last_ty, did_ret) = self.emit_block(
                    body,
                    &mut c_body,
                    &mut closure_locals,
                    &mut c_defers,
                    &mut c_locals_track,
                    false,
                    &ret_ty,
                );
                if !did_ret {
                    if ret_llvm == "void" {
                        c_body.push_str("  ret void\n");
                    } else {
                        let rv = last_val.unwrap_or_else(|| "0".to_string());
                        let rv_llvm = Self::llvm_val_type(&last_ty);
                        if rv_llvm != ret_llvm && rv_llvm == "i1" && ret_llvm == "i64" {
                            let zext = self.next_reg();
                            c_body.push_str(&format!(
                                "  {} = zext i1 {} to i64\n  ret i64 {}\n",
                                zext, rv, zext
                            ));
                        } else {
                            c_body.push_str(&format!("  ret {} {}\n", ret_llvm, rv));
                        }
                    }
                }

                let c_fn_ir = format!(
                    "define dso_local {} @{}({}) {{\nentry:\n{}{}}}\n\n",
                    ret_llvm,
                    c_fn_name,
                    param_sigs.join(", "),
                    c_entry_allocas,
                    c_body
                );
                self.ir_functions.push_str(&c_fn_ir);

                (
                    format!("@{}", c_fn_name),
                    Type::Function(param_types, Box::new(ret_ty)),
                )
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

        if let Some(lv) = locals.get(&fn_name).cloned() {
            if matches!(lv.ty, Type::Function(_, _)) || matches!(lv.ty, Type::Pointer(_)) {
                let fn_ptr = self.next_reg();
                out.push_str(&format!(
                    "  {} = load ptr, ptr {}, align 8\n",
                    fn_ptr, lv.alloca_reg
                ));
                let mut llvm_args = Vec::new();
                for a in args {
                    let (v, aty) = self.emit_expr(&a.expr, out, locals, None);
                    let alty = Self::llvm_val_type(&aty);
                    llvm_args.push(format!("{} {}", alty, v));
                }
                let ret_ty = match lv.ty {
                    Type::Function(_, ref r) => (**r).clone(),
                    _ => Type::I64,
                };
                let ret_llvm = Self::llvm_val_type(&ret_ty);
                if ret_llvm == "void" {
                    out.push_str(&format!(
                        "  call void {}({})\n",
                        fn_ptr,
                        llvm_args.join(", ")
                    ));
                    return ("0".to_string(), Type::Void);
                } else {
                    let res = self.next_reg();
                    out.push_str(&format!(
                        "  {} = call {} {}({})\n",
                        res,
                        ret_llvm,
                        fn_ptr,
                        llvm_args.join(", ")
                    ));
                    return (res, ret_ty);
                }
            }
        }

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
            let (mut lv, lt) = self.emit_expr(&args[0].expr, out, locals, None);
            let (mut rv, rt) = self.emit_expr(&args[1].expr, out, locals, None);
            if matches!(lt.strip_region(), Type::Option(_)) && !matches!(rt.strip_region(), Type::Option(_)) {
                let pp = self.next_reg();
                out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n", pp, lv));
                let loaded = self.next_reg();
                out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", loaded, pp));
                lv = loaded;
            } else if matches!(rt.strip_region(), Type::Option(_)) && !matches!(lt.strip_region(), Type::Option(_)) {
                let pp = self.next_reg();
                out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n", pp, rv));
                let loaded = self.next_reg();
                out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", loaded, pp));
                rv = loaded;
            }
            let is_str = matches!(lt.strip_region(), Type::String | Type::Str) || matches!(rt.strip_region(), Type::String | Type::Str);
            let ext = if is_str {
                let eq = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_str_eq(ptr {}, ptr {})\n", eq, lv, rv));
                eq
            } else {
                let cmp = self.next_reg();
                out.push_str(&format!("  {} = icmp eq i64 {}, {}\n", cmp, lv, rv));
                let ext = self.next_reg();
                out.push_str(&format!("  {} = zext i1 {} to i64\n", ext, cmp));
                ext
            };
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
            let pay_llvm = Self::llvm_val_type(&pay_ty);
            let i64_pay = if pay_llvm == "ptr" {
                let conv = self.next_reg();
                out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, pay_val));
                conv
            } else if pay_llvm == "double" {
                let conv = self.next_reg();
                out.push_str(&format!("  {} = bitcast double {} to i64\n", conv, pay_val));
                conv
            } else {
                pay_val
            };
            out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", i64_pay, pp));
            let ret_t = if fn_name == "Some" {
                Type::Option(Box::new(pay_ty))
            } else {
                Type::Result(Box::new(pay_ty), Box::new(Type::String))
            };
            return (eptr, ret_t);
        }

        if fn_name == "__aps_drop_vec" {
            if let Some(first) = args.first() {
                let (v, _) = self.emit_expr(&first.expr, out, locals, None);
                out.push_str(&format!("  call void @aps_vec_free(ptr {})\n", v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "time_now_ms" || fn_name == "aps_time_now_ms" {
            let r = self.next_reg();
            out.push_str(&format!("  {} = call i64 @aps_time_now_ms()\n", r));
            return (r, Type::I64);
        }
        if fn_name == "time_now_ns" || fn_name == "aps_time_now_ns" {
            let r = self.next_reg();
            out.push_str(&format!("  {} = call i64 @aps_time_now_ns()\n", r));
            return (r, Type::I64);
        }
        if fn_name == "clock_nanos" {
            let r = self.next_reg();
            out.push_str(&format!("  {} = call i64 @aps_clock_nanos()\n", r));
            return (r, Type::I64);
        }
        if fn_name == "simd_dot_f64" {
            if args.len() >= 3 {
                let (va, ty_a) = self.emit_expr(&args[0].expr, out, locals, None);
                let (vb, ty_b) = self.emit_expr(&args[1].expr, out, locals, None);
                let (len_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let va_ptr = self.unwrap_array_data_ptr(va, &ty_a, out);
                let vb_ptr = self.unwrap_array_data_ptr(vb, &ty_b, out);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call double @aps_simd_dot_f64(ptr {}, ptr {}, i64 {})\n", r, va_ptr, vb_ptr, len_v));
                return (r, Type::F64);
            }
        }
        if fn_name == "simd_sum_i64" {
            if args.len() >= 2 {
                let (va, ty_a) = self.emit_expr(&args[0].expr, out, locals, None);
                let (len_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let va_ptr = self.unwrap_array_data_ptr(va, &ty_a, out);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_simd_sum_i64(ptr {}, i64 {})\n", r, va_ptr, len_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "simd_min_i64" {
            if args.len() >= 2 {
                let (va, ty_a) = self.emit_expr(&args[0].expr, out, locals, None);
                let (len_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let va_ptr = self.unwrap_array_data_ptr(va, &ty_a, out);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_simd_min_i64(ptr {}, i64 {})\n", r, va_ptr, len_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "simd_max_i64" {
            if args.len() >= 2 {
                let (va, ty_a) = self.emit_expr(&args[0].expr, out, locals, None);
                let (len_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let va_ptr = self.unwrap_array_data_ptr(va, &ty_a, out);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_simd_max_i64(ptr {}, i64 {})\n", r, va_ptr, len_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "simd_find_byte" {
            if args.len() >= 3 {
                let (va, ty_a) = self.emit_expr(&args[0].expr, out, locals, None);
                let (len_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (byte_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let va_ptr = self.unwrap_array_data_ptr(va, &ty_a, out);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_simd_find_byte(ptr {}, i64 {}, i64 {})\n", r, va_ptr, len_v, byte_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "simd_count_byte" {
            if args.len() >= 3 {
                let (va, ty_a) = self.emit_expr(&args[0].expr, out, locals, None);
                let (len_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (byte_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let va_ptr = self.unwrap_array_data_ptr(va, &ty_a, out);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_simd_count_byte(ptr {}, i64 {}, i64 {})\n", r, va_ptr, len_v, byte_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "simd_is_ascii" {
            if args.len() >= 2 {
                let (va, ty_a) = self.emit_expr(&args[0].expr, out, locals, None);
                let (len_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let va_ptr = self.unwrap_array_data_ptr(va, &ty_a, out);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_simd_is_ascii(ptr {}, i64 {})\n", r, va_ptr, len_v));
                return (r, Type::Bool);
            }
        }
        if fn_name == "str_slice" {
            if args.len() >= 3 {
                let (va, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (start_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (len_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_str_slice(ptr {}, i64 {}, i64 {})\n", r, va, start_v, len_v));
                return (r, Type::String);
            }
        }
        if fn_name == "str_find" {
            if args.len() >= 2 {
                let (va, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (vb, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_str_find(ptr {}, ptr {})\n", r, va, vb));
                return (r, Type::I64);
            }
        }
        if fn_name == "str_find_byte" {
            if args.len() >= 2 {
                let (va, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (byte_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_str_find_byte(ptr {}, i64 {})\n", r, va, byte_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "str_view_eq" {
            if args.len() >= 4 {
                let (va, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (la, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (vb, _) = self.emit_expr(&args[2].expr, out, locals, None);
                let (lb, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_str_view_eq(ptr {}, i64 {}, ptr {}, i64 {})\n", r, va, la, vb, lb));
                return (r, Type::Bool);
            }
        }
        if fn_name == "http_response_fast" {
            if args.len() >= 3 {
                let (status_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (ctype_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (body_v, _) = self.emit_expr(&args[2].expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_http_response_fast(i64 {}, ptr {}, ptr {})\n", r, status_v, ctype_v, body_v));
                return (r, Type::String);
            }
        }
        if fn_name == "sha256_hex" {
            if let Some(first) = args.first() {
                let (va, _) = self.emit_expr(&first.expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @sha256_hex(ptr {})\n", r, va));
                return (r, Type::String);
            }
        }
        if fn_name == "parse_i64" || fn_name == "str_to_i64" {
            if let Some(first) = args.first() {
                let (va, _) = self.emit_expr(&first.expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_str_to_i64(ptr {})\n", r, va));
                return (r, Type::I64);
            }
        }
        if fn_name == "http_server_serve" || fn_name == "aps_http_server_serve" {
            if args.len() >= 3 {
                let (port_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (fn_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (max_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_http_server_serve(i64 {}, ptr {}, i64 {})\n", r, port_v, fn_v, max_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "http_client_request" || fn_name == "aps_http_client_request" {
            if args.len() >= 5 {
                let (m_v, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (h_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (port_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (p_v, _) = self.emit_expr(&args[3].expr, out, locals, None);
                let (b_v, _) = self.emit_expr(&args[4].expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_http_client_request(ptr {}, ptr {}, i64 {}, ptr {}, ptr {})\n", r, m_v, h_v, port_v, p_v, b_v));
                return (r, Type::Struct("Response".to_string(), Vec::new()));
            }
        }
        if fn_name == "gui_window_create" || fn_name == "aps_gui_window_create" {
            if args.len() >= 3 {
                let (title_v, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (w_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let r_ptr = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_gui_window_create(ptr {}, i64 {}, i64 {})\n", r_ptr, title_v, w_v, h_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", r, r_ptr));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_window_run" || fn_name == "aps_gui_window_run" {
            if let Some(first) = args.first() {
                let (win_v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_window_run(ptr {})\n", win_ptr));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_window_close" || fn_name == "aps_gui_window_close" {
            if let Some(first) = args.first() {
                let (win_v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_window_close(ptr {})\n", win_ptr));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_add_button" || fn_name == "aps_gui_add_button" {
            if args.len() >= 7 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (txt_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (x_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[5].expr, out, locals, Some(&Type::I64));
                let (fn_v, fn_ty) = self.emit_expr(&args[6].expr, out, locals, None);
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let fn_ptr = if fn_ty == Type::I64 {
                    let p = self.next_reg();
                    out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", p, fn_v));
                    p
                } else {
                    fn_v
                };
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_add_button(ptr {}, ptr {}, i64 {}, i64 {}, i64 {}, i64 {}, ptr {})\n", r, win_ptr, txt_v, x_v, y_v, w_v, h_v, fn_ptr));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_add_label" || fn_name == "aps_gui_add_label" {
            if args.len() >= 6 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (txt_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (x_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[5].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_add_label(ptr {}, ptr {}, i64 {}, i64 {}, i64 {}, i64 {})\n", r, win_ptr, txt_v, x_v, y_v, w_v, h_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_add_input" || fn_name == "aps_gui_add_input" {
            if args.len() >= 6 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (txt_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (x_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[5].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_add_input(ptr {}, ptr {}, i64 {}, i64 {}, i64 {}, i64 {})\n", r, win_ptr, txt_v, x_v, y_v, w_v, h_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_widget_set_text" || fn_name == "aps_gui_widget_set_text" {
            if args.len() >= 3 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (txt_v, _) = self.emit_expr(&args[2].expr, out, locals, None);
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_widget_set_text(ptr {}, i64 {}, ptr {})\n", win_ptr, id_v, txt_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_widget_get_text" || fn_name == "aps_gui_widget_get_text" {
            if args.len() >= 2 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_gui_widget_get_text(ptr {}, i64 {})\n", r, win_ptr, id_v));
                return (r, Type::String);
            }
        }
        if fn_name == "gui_alert" || fn_name == "aps_gui_alert" || fn_name == "alert" {
            if args.len() >= 2 {
                let (t_v, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (m_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                out.push_str(&format!("  call void @aps_gui_alert(ptr {}, ptr {})\n", t_v, m_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_confirm" || fn_name == "aps_gui_confirm" || fn_name == "confirm" {
            if args.len() >= 2 {
                let (t_v, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (m_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_confirm(ptr {}, ptr {})\n", r, t_v, m_v));
                return (r, Type::Bool);
            }
        }
        if fn_name == "gui_error" || fn_name == "aps_gui_error" || fn_name == "error" {
            if args.len() >= 2 {
                let (t_v, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (m_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                out.push_str(&format!("  call void @aps_gui_error(ptr {}, ptr {})\n", t_v, m_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_window_set_dark_mode" || fn_name == "aps_gui_window_set_dark_mode" {
            if args.len() >= 2 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (en_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::Bool));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_window_set_dark_mode(ptr {}, i64 {})\n", win_ptr, en_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_window_set_bg_color" || fn_name == "aps_gui_window_set_bg_color" {
            if args.len() >= 4 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (r_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (g_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (b_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_window_set_bg_color(ptr {}, i64 {}, i64 {}, i64 {})\n", win_ptr, r_v, g_v, b_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_add_checkbox" || fn_name == "aps_gui_add_checkbox" {
            if args.len() >= 7 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (txt_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (x_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[5].expr, out, locals, Some(&Type::I64));
                let (chk_v, _) = self.emit_expr(&args[6].expr, out, locals, Some(&Type::Bool));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_add_checkbox(ptr {}, ptr {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n", r, win_ptr, txt_v, x_v, y_v, w_v, h_v, chk_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_checkbox_is_checked" || fn_name == "aps_gui_checkbox_is_checked" {
            if args.len() >= 2 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_checkbox_is_checked(ptr {}, i64 {})\n", r, win_ptr, id_v));
                return (r, Type::Bool);
            }
        }
        if fn_name == "gui_checkbox_set_checked" || fn_name == "aps_gui_checkbox_set_checked" {
            if args.len() >= 3 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (chk_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::Bool));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_checkbox_set_checked(ptr {}, i64 {}, i64 {})\n", win_ptr, id_v, chk_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_add_progressbar" || fn_name == "aps_gui_add_progressbar" {
            if args.len() >= 5 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (x_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_add_progressbar(ptr {}, i64 {}, i64 {}, i64 {}, i64 {})\n", r, win_ptr, x_v, y_v, w_v, h_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_progressbar_set_value" || fn_name == "aps_gui_progressbar_set_value" {
            if args.len() >= 3 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (val_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_progressbar_set_value(ptr {}, i64 {}, i64 {})\n", win_ptr, id_v, val_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_add_image" || fn_name == "aps_gui_add_image" {
            if args.len() >= 6 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (path_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (x_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[5].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_add_image(ptr {}, ptr {}, i64 {}, i64 {}, i64 {}, i64 {})\n", r, win_ptr, path_v, x_v, y_v, w_v, h_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_add_webview" || fn_name == "aps_gui_add_webview" {
            if args.len() >= 6 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (url_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (x_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[5].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_add_webview(ptr {}, ptr {}, i64 {}, i64 {}, i64 {}, i64 {})\n", r, win_ptr, url_v, x_v, y_v, w_v, h_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_webview_navigate" || fn_name == "aps_gui_webview_navigate" {
            if args.len() >= 3 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let (url_v, _) = self.emit_expr(&args[2].expr, out, locals, None);
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_webview_navigate(ptr {}, i64 {}, ptr {})\n", win_ptr, id_v, url_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_audio_play" || fn_name == "aps_gui_audio_play" || fn_name == "audio_play" {
            if let Some(first) = args.first() {
                let (p_v, _) = self.emit_expr(&first.expr, out, locals, None);
                out.push_str(&format!("  call void @aps_gui_audio_play(ptr {})\n", p_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_audio_pause" || fn_name == "aps_gui_audio_pause" || fn_name == "audio_pause" {
            out.push_str("  call void @aps_gui_audio_pause()\n");
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_audio_resume" || fn_name == "aps_gui_audio_resume" || fn_name == "audio_resume" {
            out.push_str("  call void @aps_gui_audio_resume()\n");
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_audio_stop" || fn_name == "aps_gui_audio_stop" || fn_name == "audio_stop" {
            out.push_str("  call void @aps_gui_audio_stop()\n");
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_video_create" || fn_name == "aps_gui_video_create" {
            if args.len() >= 6 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (p_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let (x_v, _) = self.emit_expr(&args[2].expr, out, locals, Some(&Type::I64));
                let (y_v, _) = self.emit_expr(&args[3].expr, out, locals, Some(&Type::I64));
                let (w_v, _) = self.emit_expr(&args[4].expr, out, locals, Some(&Type::I64));
                let (h_v, _) = self.emit_expr(&args[5].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @aps_gui_video_create(ptr {}, ptr {}, i64 {}, i64 {}, i64 {}, i64 {})\n", r, win_ptr, p_v, x_v, y_v, w_v, h_v));
                return (r, Type::I64);
            }
        }
        if fn_name == "gui_video_play" || fn_name == "aps_gui_video_play" {
            if args.len() >= 2 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_video_play(ptr {}, i64 {})\n", win_ptr, id_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_video_pause" || fn_name == "aps_gui_video_pause" {
            if args.len() >= 2 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_video_pause(ptr {}, i64 {})\n", win_ptr, id_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_video_stop" || fn_name == "aps_gui_video_stop" {
            if args.len() >= 2 {
                let (win_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                let (id_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                let win_ptr = self.next_reg();
                out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", win_ptr, win_v));
                out.push_str(&format!("  call void @aps_gui_video_stop(ptr {}, i64 {})\n", win_ptr, id_v));
            }
            return ("0".to_string(), Type::Void);
        }
        if fn_name == "gui_file_open" || fn_name == "aps_gui_file_open" || fn_name == "file_open_dialog" {
            if args.len() >= 2 {
                let (t_v, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (f_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_gui_file_open(ptr {}, ptr {})\n", r, t_v, f_v));
                return (r, Type::String);
            }
        }
        if fn_name == "gui_file_save" || fn_name == "aps_gui_file_save" || fn_name == "file_save_dialog" {
            if args.len() >= 2 {
                let (t_v, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (f_v, _) = self.emit_expr(&args[1].expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @aps_gui_file_save(ptr {}, ptr {})\n", r, t_v, f_v));
                return (r, Type::String);
            }
        }
        if fn_name == "likely" {
            if let Some(first) = args.first() {
                let (v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::Bool));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @llvm.expect.i64(i64 {}, i64 1)\n", r, v));
                return (r, Type::Bool);
            }
        }
        if fn_name == "unlikely" {
            if let Some(first) = args.first() {
                let (v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::Bool));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call i64 @llvm.expect.i64(i64 {}, i64 0)\n", r, v));
                return (r, Type::Bool);
            }
        }
        if fn_name == "promote" {
            if let Some(first) = args.first() {
                let (v, actual_ty) = self.emit_expr(&first.expr, out, locals, None);
                let clean_ty = actual_ty.strip_region().clone();
                let llvm_ty = Self::llvm_val_type(&clean_ty);
                if llvm_ty == "ptr" {
                    let obj_size = match &clean_ty {
                        Type::Struct(sname, _) => {
                            self.structs.get(sname).map(|fs| fs.len().max(1) * 8).unwrap_or(32) as i64
                        }
                        Type::Array(_, Some(sz)) => (*sz * 8) as i64,
                        Type::String | Type::Str => 24i64,
                        _ => 32i64,
                    };
                    let heap_ptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 {})\n", heap_ptr, obj_size));
                    out.push_str(&format!(
                        "  call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)\n",
                        heap_ptr, v, obj_size
                    ));
                    return (heap_ptr, clean_ty);
                } else {
                    return (v, clean_ty);
                }
            }
        }
        if fn_name == "region_set_quota" {
            if args.len() >= 2 {
                let (r_ptr, _) = self.emit_expr(&args[0].expr, out, locals, None);
                let (quota_v, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                out.push_str(&format!("  call void @aps_region_set_quota(ptr {}, i64 {})\n", r_ptr, quota_v));
                return ("0".to_string(), Type::Void);
            }
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
        if fn_name == "sqrt" {
            if let Some(first) = args.first() {
                let (v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::F64));
                let r = self.next_reg();
                out.push_str(&format!("  {} = call double @aps_sqrt(double {})\n", r, v));
                return (r, Type::F64);
            }
        }
        if fn_name == "channel" {
            let cap_val = if let Some(first) = args.first() {
                self.emit_expr(&first.expr, out, locals, Some(&Type::I64)).0
            } else {
                "16".to_string()
            };
            let r = self.next_reg();
            out.push_str(&format!("  {} = call ptr @channel(i64 {})\n", r, cap_val));
            return (r, Type::Channel(Box::new(Type::Unknown)));
        }
        if fn_name == "spawn_thread" {
            if let Some(first) = args.first() {
                let (v, _) = self.emit_expr(&first.expr, out, locals, None);
                let r = self.next_reg();
                out.push_str(&format!("  {} = call ptr @spawn_thread(ptr {})\n", r, v));
                return (r, Type::Task(Box::new(Type::Unknown)));
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
                        let pp = self.next_reg();
                        out.push_str(&format!(
                            "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n",
                            pp, eptr
                        ));
                        let pt_llvm = Self::llvm_val_type(&pt);
                        let i64_pv = if pt_llvm == "ptr" {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, pv));
                            conv
                        } else if pt_llvm == "double" {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = bitcast double {} to i64\n", conv, pv));
                            conv
                        } else {
                            pv
                        };
                        out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", i64_pv, pp));
                    }
                    if let Some(second) = args.get(1) {
                        let (pv, pt) = self.emit_expr(&second.expr, out, locals, None);
                        let pp = self.next_reg();
                        out.push_str(&format!(
                            "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 2\n",
                            pp, eptr
                        ));
                        let pt_llvm = Self::llvm_val_type(&pt);
                        let i64_pv = if pt_llvm == "ptr" {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, pv));
                            conv
                        } else if pt_llvm == "double" {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = bitcast double {} to i64\n", conv, pv));
                            conv
                        } else {
                            pv
                        };
                        out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", i64_pv, pp));
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
                            let pp = self.next_reg();
                            out.push_str(&format!(
                                "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n",
                                pp, eptr
                            ));
                            let pt_llvm = Self::llvm_val_type(&pt);
                            let i64_pv = if pt_llvm == "ptr" {
                                let conv = self.next_reg();
                                out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, pv));
                                conv
                            } else if pt_llvm == "double" {
                                let conv = self.next_reg();
                                out.push_str(&format!("  {} = bitcast double {} to i64\n", conv, pv));
                                conv
                            } else {
                                pv
                            };
                            out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", i64_pv, pp));
                        }
                        if let Some(second) = args.get(1) {
                            let (pv, pt) = self.emit_expr(&second.expr, out, locals, None);
                            let pp = self.next_reg();
                            out.push_str(&format!(
                                "  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 2\n",
                                pp, eptr
                            ));
                            let pt_llvm = Self::llvm_val_type(&pt);
                            let i64_pv = if pt_llvm == "ptr" {
                                let conv = self.next_reg();
                                out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, pv));
                                conv
                            } else if pt_llvm == "double" {
                                let conv = self.next_reg();
                                out.push_str(&format!("  {} = bitcast double {} to i64\n", conv, pv));
                                conv
                            } else {
                                pv
                            };
                            out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", i64_pv, pp));
                        }
                        return (eptr, Type::Enum(tname.clone(), Vec::new()));
                    }
                }
                if tname == "Vec" && (method == "new" || method == "with_capacity") {
                    let mut cap_val = 16i64;
                    if method == "with_capacity" {
                        if let Some(first) = args.first() {
                            if let ExprKind::IntLit(n, _) = &first.expr.kind {
                                cap_val = (*n).max(4);
                            }
                        }
                    }
                    let vec_ptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", vec_ptr));
                    let data_ptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 {})\n", data_ptr, cap_val * 8));
                    let cap_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 1\n",
                        cap_p, vec_ptr
                    ));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", cap_val, cap_p));
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
                if (tname == "HashMap" || tname.starts_with("HashMap$")) && (method == "new" || method == "with_capacity") {
                    let (k_ty, v_ty) = if tname.starts_with("HashMap$") {
                        let parts: Vec<&str> = tname.split('$').collect();
                        let k = parts.get(1).map(|t| match *t {
                            "String" | "str" => Type::String,
                            "f32" | "f64" => Type::F64,
                            "bool" => Type::Bool,
                            "char" => Type::Char,
                            _ => Type::I64,
                        }).unwrap_or(Type::String);
                        let v = parts.get(2).map(|t| match *t {
                            "String" | "str" => Type::String,
                            "f32" | "f64" => Type::F64,
                            "bool" => Type::Bool,
                            "char" => Type::Char,
                            _ => Type::I64,
                        }).unwrap_or(Type::I64);
                        (k, v)
                    } else {
                        (Type::String, Type::I64)
                    };
                    let is_str_key = match &k_ty {
                        Type::String | Type::Str => 1i64,
                        _ => 0i64,
                    };
                    let m = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_map_new(i64 {})\n", m, is_str_key));
                    return (m, Type::HashMap(Box::new(k_ty), Box::new(v_ty)));
                }
                if tname == "AtomicI64" && method == "new" {
                    let init_val = if let Some(first) = args.first() {
                        let (iv, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::I64));
                        iv
                    } else {
                        "0".to_string()
                    };
                    let a_ptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 8)\n", a_ptr));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", init_val, a_ptr));
                    return (a_ptr, Type::Struct("AtomicI64".to_string(), Vec::new()));
                }
                if tname == "Mutex" && method == "new" {
                    if let Some(first) = args.first() {
                        let (val, ty) = self.emit_expr(&first.expr, out, locals, None);
                        return (val, Type::Mutex(Box::new(ty)));
                    }
                }
                if tname == "File" && method == "create" {
                    if let Some(first) = args.first() {
                        let (path_v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::String));
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_file_create(ptr {})\n", r, path_v));
                        return (r, Type::Result(Box::new(Type::Pointer(Box::new(Type::I8))), Box::new(Type::String)));
                    }
                }
                if tname == "File" && method == "open" {
                    if let Some(first) = args.first() {
                        let (path_v, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::String));
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_file_open(ptr {})\n", r, path_v));
                        return (r, Type::Result(Box::new(Type::Pointer(Box::new(Type::I8))), Box::new(Type::String)));
                    }
                }
                if tname == "TcpListener" && method == "bind" {
                    let host_v = if let Some(first) = args.first() {
                        self.emit_expr(&first.expr, out, locals, Some(&Type::String)).0
                    } else { "null".to_string() };
                    let port_v = if let Some(sec) = args.get(1) {
                        self.emit_expr(&sec.expr, out, locals, Some(&Type::I64)).0
                    } else { "8080".to_string() };
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_tcp_listener_bind(ptr {}, i64 {})\n", r, host_v, port_v));
                    return (r, Type::Result(Box::new(Type::I64), Box::new(Type::String)));
                }
                if tname == "TcpStream" && method == "connect" {
                    let host_v = if let Some(first) = args.first() {
                        self.emit_expr(&first.expr, out, locals, Some(&Type::String)).0
                    } else { "null".to_string() };
                    let port_v = if let Some(sec) = args.get(1) {
                        self.emit_expr(&sec.expr, out, locals, Some(&Type::I64)).0
                    } else { "80".to_string() };
                    let timeout_v = if let Some(third) = args.get(2) {
                        self.emit_expr(&third.expr, out, locals, Some(&Type::I64)).0
                    } else { "1000".to_string() };
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_tcp_stream_connect(ptr {}, i64 {}, i64 {})\n", r, host_v, port_v, timeout_v));
                    return (r, Type::Result(Box::new(Type::I64), Box::new(Type::String)));
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
                    let inner_llvm = Self::llvm_val_type(&inner_ty);
                    let obj_size = match &inner_ty {
                        Type::Struct(sname, _) => {
                            self.structs.get(sname).map(|fs| fs.len().max(1) * 8).unwrap_or(32) as i64
                        }
                        _ => 8i64,
                    };
                    let reg_mem = self.next_reg();
                    out.push_str(&format!(
                        "  {} = call ptr @aps_region_alloc(ptr {}, i64 {})\n",
                        reg_mem, recv_val, obj_size
                    ));
                    if inner_llvm == "ptr" {
                        out.push_str(&format!(
                            "  call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)\n",
                            reg_mem, val, obj_size
                        ));
                    } else if inner_llvm == "double" {
                        out.push_str(&format!("  store double {}, ptr {}, align 8\n", val, reg_mem));
                    } else {
                        out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", val, reg_mem));
                    }
                    if inner_llvm == "ptr" {
                        return (
                            reg_mem,
                            Type::RegionBound {
                                region: rname.clone(),
                                inner: Box::new(inner_ty),
                            },
                        );
                    } else {
                        return (val, inner_ty);
                    }
                }
            }
            Type::String | Type::Str => {
                if method == "len" || method == "byte_len" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_len(ptr {})\n", r, recv_val));
                    return (r, Type::I64);
                }
                if method == "char_count" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_char_count(ptr {})\n", r, recv_val));
                    return (r, Type::I64);
                }
                if method == "is_empty" {
                    let l = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_len(ptr {})\n", l, recv_val));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", cmp, l));
                    let ext = self.next_reg();
                    out.push_str(&format!("  {} = zext i1 {} to i64\n", ext, cmp));
                    return (ext, Type::Bool);
                }
                if method == "contains" && !args.is_empty() {
                    let (sub_val, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::String));
                    let f = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_find(ptr {}, ptr {})\n", f, recv_val, sub_val));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp sge i64 {}, 0\n", cmp, f));
                    let ext = self.next_reg();
                    out.push_str(&format!("  {} = zext i1 {} to i64\n", ext, cmp));
                    return (ext, Type::Bool);
                }
                if (method == "slice_bytes" || method == "slice") && args.len() >= 2 {
                    let (s_val, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                    let (len_val, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_str_slice(ptr {}, i64 {}, i64 {})\n", r, recv_val, s_val, len_val));
                    return (r, Type::String);
                }
                if method == "starts_with" && !args.is_empty() {
                    let (prefix_val, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::String));
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_starts_with(ptr {}, ptr {})\n", r, recv_val, prefix_val));
                    return (r, Type::Bool);
                }
                if method == "ends_with" && !args.is_empty() {
                    let (suffix_val, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::String));
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_ends_with(ptr {}, ptr {})\n", r, recv_val, suffix_val));
                    return (r, Type::Bool);
                }
                if method == "to_i64" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_str_to_i64(ptr {})\n", r, recv_val));
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
                if (method == "get_copy" || method == "get") && !args.is_empty() {
                    let (idx_v, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                    let data_field = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                        data_field, recv_val
                    ));
                    let data_ptr = self.next_reg();
                    out.push_str(&format!("  {} = load ptr, ptr {}, align 8\n", data_ptr, data_field));
                    let elem_llvm = Self::llvm_val_type(elem);
                    let elem_ptr = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds {}, ptr {}, i64 {}\n",
                        elem_ptr, elem_llvm, data_ptr, idx_v
                    ));
                    let val = self.next_reg();
                    out.push_str(&format!("  {} = load {}, ptr {}, align 8\n", val, elem_llvm, elem_ptr));
                    return (val, elem.as_ref().clone());
                }
                if method == "slice" && args.len() >= 2 {
                    let (s_val, _) = self.emit_expr(&args[0].expr, out, locals, Some(&Type::I64));
                    let (e_val, _) = self.emit_expr(&args[1].expr, out, locals, Some(&Type::I64));
                    let diff = self.next_reg();
                    out.push_str(&format!("  {} = sub i64 {}, {}\n", diff, e_val, s_val));
                    let is_pos = self.next_reg();
                    out.push_str(&format!("  {} = icmp sgt i64 {}, 0\n", is_pos, diff));
                    let slice_len = self.next_reg();
                    out.push_str(&format!("  {} = select i1 {}, i64 {}, i64 0\n", slice_len, is_pos, diff));
                    let data_field = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                        data_field, recv_val
                    ));
                    let data_ptr = self.next_reg();
                    out.push_str(&format!("  {} = load ptr, ptr {}, align 8\n", data_ptr, data_field));
                    let elem_llvm = Self::llvm_val_type(elem);
                    let offset_data = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds {}, ptr {}, i64 {}\n",
                        offset_data, elem_llvm, data_ptr, s_val
                    ));
                    let slice_vec = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", slice_vec));
                    let len_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 0\n",
                        len_p, slice_vec
                    ));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", slice_len, len_p));
                    let cap_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 1\n",
                        cap_p, slice_vec
                    ));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", slice_len, cap_p));
                    let dat_p = self.next_reg();
                    out.push_str(&format!(
                        "  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 2\n",
                        dat_p, slice_vec
                    ));
                    out.push_str(&format!("  store ptr {}, ptr {}, align 8\n", offset_data, dat_p));
                    return (slice_vec, Type::Array(elem.clone(), None));
                }
                if method == "push" {
                    if let Some(first) = args.first() {
                        let (ev, actual_ty) = self.emit_expr(&first.expr, out, locals, Some(elem));
                        let elem_llvm = Self::llvm_val_type(&actual_ty);
                        let i64_ev = if elem_llvm == "ptr" {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, ev));
                            conv
                        } else if elem_llvm == "double" {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = bitcast double {} to i64\n", conv, ev));
                            conv
                        } else {
                            ev
                        };
                        out.push_str(&format!(
                            "  call void @aps_vec_push(ptr {}, i64 {})\n",
                            recv_val, i64_ev
                        ));
                        return ("0".to_string(), Type::Void);
                    }
                }
                if method == "pop" {
                    let len_p = self.next_reg();
                    let cur_len = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Vec, ptr {}, i32 0, i32 0\n", len_p, recv_val));
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", cur_len, len_p));
                    let is_empty = self.next_reg();
                    out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_empty, cur_len));
                    let res_opt = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", res_opt));
                    let tag_p = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n", tag_p, res_opt));
                    let val_p = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n", val_p, res_opt));
                    let empty_lbl = self.next_label("vec_pop_empty");
                    let non_empty_lbl = self.next_label("vec_pop_val");
                    let done_lbl = self.next_label("vec_pop_done");
                    out.push_str(&format!("  br i1 {}, label %{}, label %{}\n", is_empty, empty_lbl, non_empty_lbl));
                    out.push_str(&format!("{}:\n", empty_lbl));
                    out.push_str(&format!("  store i64 0, ptr {}, align 8\n", tag_p));
                    out.push_str(&format!("  store i64 0, ptr {}, align 8\n", val_p));
                    out.push_str(&format!("  br label %{}\n", done_lbl));
                    out.push_str(&format!("{}:\n", non_empty_lbl));
                    let popped = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_vec_pop(ptr {})\n", popped, recv_val));
                    out.push_str(&format!("  store i64 1, ptr {}, align 8\n", tag_p));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", popped, val_p));
                    out.push_str(&format!("  br label %{}\n", done_lbl));
                    out.push_str(&format!("{}:\n", done_lbl));
                    return (res_opt, Type::Option(Box::new(elem.as_ref().clone())));
                }
            }
            Type::Channel(inner) => {
                if method == "send" {
                    if let Some(first) = args.first() {
                        let (v, actual_ty) = self.emit_expr(&first.expr, out, locals, Some(inner));
                        let v_llvm = Self::llvm_val_type(&actual_ty);
                        let ptr_v = if v_llvm == "ptr" {
                            v
                        } else if v_llvm == "double" {
                            let c1 = self.next_reg();
                            out.push_str(&format!("  {} = bitcast double {} to i64\n", c1, v));
                            let c2 = self.next_reg();
                            out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", c2, c1));
                            c2
                        } else {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", conv, v));
                            conv
                        };
                        out.push_str(&format!(
                            "  call void @aps_channel_send(ptr {}, ptr {})\n",
                            recv_val, ptr_v
                        ));
                        return ("0".to_string(), Type::Void);
                    }
                }
                if method == "recv" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_channel_recv(ptr {})\n", r, recv_val));
                    return (r, Type::Option(inner.clone()));
                }
                if method == "close" {
                    out.push_str(&format!("  call void @aps_channel_close(ptr {})\n", recv_val));
                    return ("0".to_string(), Type::Void);
                }
            }
            Type::Task(_) => {
                if method == "join" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_thread_join(ptr {})\n", r, recv_val));
                    return (r, Type::I64);
                }
            }
            Type::Option(inner) => {
                if method == "is_some" {
                    let tp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n", tp, recv_val));
                    let tag = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", tag, tp));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp eq i64 {}, 1\n", cmp, tag));
                    return (cmp, Type::Bool);
                }
                if method == "is_none" {
                    let tp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n", tp, recv_val));
                    let tag = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", tag, tp));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", cmp, tag));
                    return (cmp, Type::Bool);
                }
                if method == "unwrap" || method == "unwrap_or" {
                    let pp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n", pp, recv_val));
                    let i64_val = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", i64_val, pp));
                    let inner_llvm = Self::llvm_val_type(inner);
                    if inner_llvm == "ptr" {
                        let conv = self.next_reg();
                        out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", conv, i64_val));
                        return (conv, inner.as_ref().clone());
                    } else if inner_llvm == "double" {
                        let conv = self.next_reg();
                        out.push_str(&format!("  {} = bitcast i64 {} to double\n", conv, i64_val));
                        return (conv, inner.as_ref().clone());
                    } else {
                        return (i64_val, inner.as_ref().clone());
                    }
                }
            }
            Type::Result(ok, _) => {
                if method == "is_ok" {
                    let tp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n", tp, recv_val));
                    let tag = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", tag, tp));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp eq i64 {}, 1\n", cmp, tag));
                    return (cmp, Type::Bool);
                }
                if method == "is_err" {
                    let tp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n", tp, recv_val));
                    let tag = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", tag, tp));
                    let cmp = self.next_reg();
                    out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", cmp, tag));
                    return (cmp, Type::Bool);
                }
                if method == "unwrap" || method == "unwrap_or" {
                    let pp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n", pp, recv_val));
                    let i64_val = self.next_reg();
                    out.push_str(&format!("  {} = load i64, ptr {}, align 8\n", i64_val, pp));
                    let ok_llvm = Self::llvm_val_type(ok);
                    if ok_llvm == "ptr" {
                        let conv = self.next_reg();
                        out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", conv, i64_val));
                        return (conv, ok.as_ref().clone());
                    } else if ok_llvm == "double" {
                        let conv = self.next_reg();
                        out.push_str(&format!("  {} = bitcast i64 {} to double\n", conv, i64_val));
                        return (conv, ok.as_ref().clone());
                    } else {
                        return (i64_val, ok.as_ref().clone());
                    }
                }
            }
            Type::HashMap(k, v) => {
                if method == "insert" && args.len() >= 2 {
                    let (kv, kty) = self.emit_expr(&args[0].expr, out, locals, Some(k));
                    let (vv, vty) = self.emit_expr(&args[1].expr, out, locals, Some(v));
                    let k_i64 = if Self::llvm_val_type(&kty) == "ptr" {
                        let c = self.next_reg();
                        out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", c, kv));
                        c
                    } else {
                        kv
                    };
                    let v_i64 = if Self::llvm_val_type(&vty) == "ptr" {
                        let c = self.next_reg();
                        out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", c, vv));
                        c
                    } else {
                        vv
                    };
                    out.push_str(&format!("  call void @aps_map_insert(ptr {}, i64 {}, i64 {})\n", recv_val, k_i64, v_i64));
                    return ("0".to_string(), Type::Void);
                }
                if method == "get" && !args.is_empty() {
                    let (kv, kty) = self.emit_expr(&args[0].expr, out, locals, Some(k));
                    let k_i64 = if Self::llvm_val_type(&kty) == "ptr" {
                        let c = self.next_reg();
                        out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", c, kv));
                        c
                    } else {
                        kv
                    };
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_map_get(ptr {}, i64 {})\n", r, recv_val, k_i64));
                    return (r, Type::Option(v.clone()));
                }
                if method == "len" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_map_len(ptr {})\n", r, recv_val));
                    return (r, Type::I64);
                }
            }
            Type::Shared(inner) => {
                if method == "clone" {
                    return (recv_val, Type::Shared(inner.clone()));
                }
                if method == "downgrade" {
                    return (recv_val, Type::Weak(inner.clone()));
                }
                if method == "lock" {
                    return (recv_val, inner.as_ref().clone());
                }
                if method == "fetch_add" {
                    let delta = if let Some(first) = args.first() {
                        let (dv, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::I64));
                        dv
                    } else { "1".to_string() };
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = atomicrmw add ptr {}, i64 {} seq_cst\n", r, recv_val, delta));
                    return (r, Type::I64);
                }
                if method == "load" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = load atomic i64, ptr {} seq_cst, align 8\n", r, recv_val));
                    return (r, Type::I64);
                }
            }
            Type::Weak(inner) => {
                if method == "upgrade" {
                    let eptr = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_alloc(i64 24)\n", eptr));
                    let tp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 0\n", tp, eptr));
                    out.push_str(&format!("  store i64 1, ptr {}, align 8\n", tp));
                    let pp = self.next_reg();
                    out.push_str(&format!("  {} = getelementptr inbounds %aps.Enum, ptr {}, i32 0, i32 1\n", pp, eptr));
                    let conv = self.next_reg();
                    out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, recv_val));
                    out.push_str(&format!("  store i64 {}, ptr {}, align 8\n", conv, pp));
                    return (eptr, Type::Option(Box::new(Type::Shared(inner.clone()))));
                }
            }
            Type::Mutex(inner) => {
                if method == "lock" {
                    return (recv_val, inner.as_ref().clone());
                }
            }
            Type::Struct(sname, _) => {
                if method == "clone" {
                    return (recv_val, Type::Struct(sname.clone(), Vec::new()));
                }
                if method == "unlock" {
                    return ("0".to_string(), Type::Void);
                }
                if sname == "TcpStream" || sname == "TcpListener" {
                    let fd_i64 = if Self::llvm_val_type(&recv_ty) == "ptr" {
                        let conv = self.next_reg();
                        out.push_str(&format!("  {} = ptrtoint ptr {} to i64\n", conv, recv_val));
                        conv
                    } else {
                        recv_val.clone()
                    };
                    if method == "write_str" {
                        let str_v = if let Some(first) = args.first() {
                            self.emit_expr(&first.expr, out, locals, Some(&Type::String)).0
                        } else { "null".to_string() };
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_tcp_stream_write_str(i64 {}, ptr {})\n", r, fd_i64, str_v));
                        return (r, Type::Result(Box::new(Type::I64), Box::new(Type::String)));
                    }
                    if method == "flush" {
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_tcp_stream_flush(i64 {})\n", r, fd_i64));
                        return (r, Type::Result(Box::new(Type::Void), Box::new(Type::String)));
                    }
                    if method == "read_line" {
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_tcp_stream_read_line(i64 {})\n", r, fd_i64));
                        return (r, Type::Result(Box::new(Type::String), Box::new(Type::String)));
                    }
                    if method == "close" {
                        out.push_str(&format!("  call void @aps_tcp_close(i64 {})\n", fd_i64));
                        return ("0".to_string(), Type::Void);
                    }
                }
                if sname == "File" {
                    if method == "write_str" {
                        let str_v = if let Some(first) = args.first() {
                            self.emit_expr(&first.expr, out, locals, Some(&Type::String)).0
                        } else { "null".to_string() };
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_file_write_str(ptr {}, ptr {})\n", r, recv_val, str_v));
                        return (r, Type::Result(Box::new(Type::I64), Box::new(Type::String)));
                    }
                    if method == "flush" {
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_file_flush(ptr {})\n", r, recv_val));
                        return (r, Type::Result(Box::new(Type::Void), Box::new(Type::String)));
                    }
                    if method == "read_to_string" {
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = call ptr @aps_file_read_to_string(ptr {})\n", r, recv_val));
                        return (r, Type::Result(Box::new(Type::String), Box::new(Type::String)));
                    }
                    if method == "close" {
                        out.push_str(&format!("  call void @aps_file_close(ptr {})\n", recv_val));
                        return ("0".to_string(), Type::Void);
                    }
                }
                if sname == "AtomicI64" {
                    if method == "fetch_add" {
                        let delta = if let Some(first) = args.first() {
                            let (dv, _) = self.emit_expr(&first.expr, out, locals, Some(&Type::I64));
                            dv
                        } else { "1".to_string() };
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = atomicrmw add ptr {}, i64 {} seq_cst\n", r, recv_val, delta));
                        return (r, Type::I64);
                    }
                    if method == "load" {
                        let r = self.next_reg();
                        out.push_str(&format!("  {} = load atomic i64, ptr {} seq_cst, align 8\n", r, recv_val));
                        return (r, Type::I64);
                    }
                }
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
            _ => {
                if method == "join" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call i64 @aps_thread_join(ptr {})\n", r, recv_val));
                    return (r, Type::I64);
                }
                if method == "send" {
                    if let Some(first) = args.first() {
                        let (v, actual_ty) = self.emit_expr(&first.expr, out, locals, None);
                        let v_llvm = Self::llvm_val_type(&actual_ty);
                        let ptr_v = if v_llvm == "ptr" {
                            v
                        } else {
                            let conv = self.next_reg();
                            out.push_str(&format!("  {} = inttoptr i64 {} to ptr\n", conv, v));
                            conv
                        };
                        out.push_str(&format!(
                            "  call void @aps_channel_send(ptr {}, ptr {})\n",
                            recv_val, ptr_v
                        ));
                        return ("0".to_string(), Type::Void);
                    }
                }
                if method == "recv" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_channel_recv(ptr {})\n", r, recv_val));
                    return (r, Type::Option(Box::new(Type::Unknown)));
                }
                if method == "write_str" {
                    let str_v = if let Some(first) = args.first() {
                        self.emit_expr(&first.expr, out, locals, Some(&Type::String)).0
                    } else { "null".to_string() };
                    let r_llvm = Self::llvm_val_type(&recv_ty);
                    let r = self.next_reg();
                    if r_llvm == "i64" {
                        out.push_str(&format!("  {} = call ptr @aps_tcp_stream_write_str(i64 {}, ptr {})\n", r, recv_val, str_v));
                    } else {
                        out.push_str(&format!("  {} = call ptr @aps_file_write_str(ptr {}, ptr {})\n", r, recv_val, str_v));
                    }
                    return (r, Type::Result(Box::new(Type::I64), Box::new(Type::String)));
                }
                if method == "flush" {
                    let r_llvm = Self::llvm_val_type(&recv_ty);
                    let r = self.next_reg();
                    if r_llvm == "i64" {
                        out.push_str(&format!("  {} = call ptr @aps_tcp_stream_flush(i64 {})\n", r, recv_val));
                    } else {
                        out.push_str(&format!("  {} = call ptr @aps_file_flush(ptr {})\n", r, recv_val));
                    }
                    return (r, Type::Result(Box::new(Type::Void), Box::new(Type::String)));
                }
                if method == "read_to_string" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_file_read_to_string(ptr {})\n", r, recv_val));
                    return (r, Type::Result(Box::new(Type::String), Box::new(Type::String)));
                }
                if method == "read_line" {
                    let r = self.next_reg();
                    out.push_str(&format!("  {} = call ptr @aps_tcp_stream_read_line(i64 {})\n", r, recv_val));
                    return (r, Type::Result(Box::new(Type::String), Box::new(Type::String)));
                }
                if method == "close" {
                    let r_llvm = Self::llvm_val_type(&recv_ty);
                    if r_llvm == "i64" {
                        out.push_str(&format!("  call void @aps_tcp_close(i64 {})\n", recv_val));
                    } else if matches!(recv_ty.strip_region(), Type::Channel(_)) {
                        out.push_str(&format!("  call void @aps_channel_close(ptr {})\n", recv_val));
                    } else {
                        out.push_str(&format!("  call void @aps_file_close(ptr {})\n", recv_val));
                    }
                    return ("0".to_string(), Type::Void);
                }
                if method == "unlock" {
                    return ("0".to_string(), Type::Void);
                }
                if method == "clone" {
                    return (recv_val, recv_ty.clone());
                }
            }
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

        let rt_obj_path = build_dir.join("aps_runtime.obj");
        let need_recompile_rt = !rt_obj_path.exists() || {
            let rt_mtime = fs::metadata(&rt_path).and_then(|m| m.modified()).ok();
            let obj_mtime = fs::metadata(&rt_obj_path).and_then(|m| m.modified()).ok();
            match (rt_mtime, obj_mtime) {
                (Some(rt_t), Some(obj_t)) => rt_t > obj_t,
                _ => true,
            }
        };

        if need_recompile_rt {
            let mut rt_cmd = Command::new(&clang);
            rt_cmd.arg(opt_flag);
            if opt_level >= 2 {
                rt_cmd.arg("-flto").arg("-march=native");
            }
            rt_cmd.arg("-c").arg(&rt_path).arg("-o").arg(&rt_obj_path);
            if std::env::var("APS_ALLOC_STATS").map(|v| v == "1").unwrap_or(false) {
                rt_cmd.arg("-DAPS_ALLOC_STATS");
            }
            let rt_out = rt_cmd.output().map_err(|e| format!("failed to compile runtime: {}", e))?;
            if !rt_out.status.success() {
                let stderr = String::from_utf8_lossy(&rt_out.stderr);
                return Err(format!("error[E0702]: LLVM runtime compilation failed:\n{}", stderr));
            }
        }

        let mut cmd = Command::new(&clang);
        cmd.arg(opt_flag)
            .arg("-Wno-override-module");

        if self.debug_info {
            cmd.arg("-g");
            if self.target_triple.contains("windows") {
                cmd.arg("-gcodeview");
            }
        }

        if std::env::var("APS_ALLOC_STATS").map(|v| v == "1").unwrap_or(false) {
            cmd.arg("-DAPS_ALLOC_STATS");
        }

        if opt_level >= 2 {
            cmd.arg("-march=native")
                .arg("-flto")
                .arg("-fuse-ld=lld")
                .arg("-fvectorize")
                .arg("-fslp-vectorize")
                .arg("-fomit-frame-pointer");
        }

        cmd.arg(&ll_path)
            .arg(&rt_obj_path);

        if cfg!(windows) {
            cmd.arg("-lws2_32")
                .arg("-luser32")
                .arg("-lgdi32")
                .arg("-lcomctl32")
                .arg("-lwinmm")
                .arg("-lcomdlg32")
                .arg("-ldwmapi")
                .arg("-lole32")
                .arg("-loleaut32");
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
