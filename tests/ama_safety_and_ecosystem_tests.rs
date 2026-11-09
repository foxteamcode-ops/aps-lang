use aps::compiler::CompilerPipeline;
use aps::tools::apm::{run_apm_command, PackageManifest};
use aps::tools::docgen::generate_markdown_docs;
use aps::tools::fmt::format_aps_source;
use aps::tools::lint::lint_program;
use aps::tools::lsp::ApsLanguageServer;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn check_code(src: &str) -> aps::compiler::CompilationResult {
    let pipeline = CompilerPipeline::new(2);
    pipeline.check_source("test.aps", src)
}

#[test]
fn test_ama_safety_01_read_only_parameter_mutation_rejected_e0042() {
    let src = r#"
struct Driver {
    id: i64
    name: String
    balance: i64
}

fn attempt_hack(driver: read Driver) {
    driver.balance += 100
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0042");
    let rendered = res.diagnostics.render_all(src);
    assert!(rendered.contains("error[E0042]: cannot modify a read-only value"));
    assert!(rendered.contains("cannot modify through read access"));
}

#[test]
fn test_ama_safety_02_immutable_let_mutation_rejected_e0042() {
    let src = r#"
fn main() {
    let balance: i64 = 500
    balance += 100
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0042");
}

#[test]
fn test_ama_safety_03_shared_frozen_mutation_rejected_e0042() {
    let src = r#"
struct Config {
    port: i64
}

fn main() {
    let cfg = Config { port: 8080 }
    let frozen = share(cfg)
    frozen.port = 9090
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0042");
}

#[test]
fn test_ama_safety_04_use_after_take_move_rejected_e0201() {
    let src = r#"
struct Packet {
    payload: String
}

fn drop_packet(p: take Packet) {
    println(p.payload)
}

fn main() {
    let pkt = Packet { payload: "data" }
    drop_packet(take pkt)
    println(pkt.payload)
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0201");
}

#[test]
fn test_ama_safety_05_double_free_via_double_take_rejected_e0201() {
    let src = r#"
struct Buffer {
    tag: String
}

fn free_buf(b: take Buffer) {}

fn main() {
    let buf = Buffer { tag: "heap_blk" }
    free_buf(take buf)
    free_buf(take buf)
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0201");
}

#[test]
fn test_ama_safety_06_move_out_of_borrowed_parameter_rejected_e0203() {
    let src = r#"
struct SecretKey {
    raw: String
}

fn sink(k: take SecretKey) {}

fn inspect(k: read SecretKey) {
    sink(take k)
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0203");
}

#[test]
fn test_ama_safety_07_conflicting_write_and_read_capabilities_rejected_e0302() {
    let src = r#"
struct Driver {
    id: i64
    name: String
    balance: i64
}

fn process_both(w: write Driver, r: read Driver) {
    w.balance += r.balance
}

fn main() {
    var d = Driver { id: 1, name: "Alibek", balance: 1000 }
    process_both(write d, read d)
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0302");
}

#[test]
fn test_ama_safety_08_field_disjoint_borrowing_allowed() {
    let src = r#"
struct Telemetry {
    rx_bytes: i64
    tx_bytes: i64
}

fn update_counters(rx: write i64, tx: read i64) {
    rx += tx
}

fn main() {
    var t = Telemetry { rx_bytes: 100, tx_bytes: 50 }
    update_counters(write t.rx_bytes, read t.tx_bytes)
}
"#;
    let res = check_code(src);
    assert!(!res.diagnostics.has_errors());
}

#[test]
fn test_ama_safety_09_region_escape_to_outer_variable_rejected_e0303() {
    let src = r#"
struct Frame {
    seq: i64
}

fn main() {
    var outer_frame = Frame { seq: 0 }
    region scratch {
        let f = scratch.alloc(Frame { seq: 99 })
        outer_frame = f
    }
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0303");
}

#[test]
fn test_ama_safety_10_region_escape_via_return_rejected_e0303() {
    let src = r#"
struct Node {
    val: i64
}

fn bad_factory() -> Node {
    region temp {
        let n = temp.alloc(Node { val: 7 })
        return n
    }
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0303");
}

#[test]
fn test_ama_safety_11_concurrent_spawn_region_capture_rejected_e0401() {
    let src = r#"
struct Job {
    id: i64
}

fn run_job(j: read Job) -> i64 {
    return j.id
}

fn main() {
    region local_arena {
        let job = local_arena.alloc(Job { id: 1 })
        let t = spawn run_job(read job)
    }
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert_eq!(res.diagnostics.diagnostics[0].code, "E0401");
}

#[test]
fn test_ama_safety_12_ffi_extern_requires_unsafe_block_e0601() {
    let bad_src = r#"
extern "C" {
    fn abs(x: i32) -> i32
}

fn main() {
    let v = abs(-10)
}
"#;
    let res_bad = check_code(bad_src);
    assert!(res_bad.diagnostics.has_errors());
    assert_eq!(res_bad.diagnostics.diagnostics[0].code, "E0601");

    let good_src = r#"
extern "C" {
    fn abs(x: i32) -> i32
}

fn main() {
    unsafe {
        let v = abs(-10)
        println(v)
    }
}
"#;
    let res_good = check_code(good_src);
    assert!(!res_good.diagnostics.has_errors());
}

#[test]
fn test_native_execution_and_bounds_check_protection() {
    let src = r#"
struct Driver {
    id: i64
    name: String
    balance: i64
}

fn add_balance(d: write Driver, delta: i64) {
    d.balance += delta
}

fn main() {
    var d = Driver { id: 7, name: "Alibek", balance: 1000 }
    add_balance(write d, 250)
    assert_eq(d.balance, 1250)
    println("NATIVE_OK")
}
"#;
    let out_dir = PathBuf::from("target/test_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe_path = out_dir.join(if cfg!(windows) {
        "native_check.exe"
    } else {
        "native_check"
    });
    let pipeline = CompilerPipeline::new(3);
    pipeline
        .build_executable("native_check.aps", src, &exe_path)
        .expect("should compile native binary");
    let output = Command::new(&exe_path)
        .output()
        .expect("should execute native binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("NATIVE_OK"));
}

#[test]
fn test_developer_tools_fmt_lint_doc_lsp_and_apm() {
    let unformatted = "struct driver {\nid: i64;\n}\nfn BadName() {\nlet x = 10;\n}\n";
    let formatted = format_aps_source(unformatted);
    assert!(formatted.contains("    id: i64"));

    let res = check_code(unformatted);
    let lint_bag = lint_program(&res.program);
    assert!(lint_bag.warning_count() >= 2); // W0001 + W0002

    let docs = generate_markdown_docs("test_mod", &res.program);
    assert!(docs.contains("# APS API Documentation"));

    let mut lsp = ApsLanguageServer::new();
    let init_resp = lsp
        .handle_json_rpc_message(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)
        .unwrap();
    assert!(init_resp.contains("aps-lsp"));

    // Test apm manifest and lockfile
    let tmp_pkg = PathBuf::from("target/test_apm_pkg");
    let _ = fs::remove_dir_all(&tmp_pkg);
    fs::create_dir_all(&tmp_pkg).unwrap();
    let init_out = run_apm_command(&["init".to_string(), "demo_pkg".to_string()], &tmp_pkg).unwrap();
    assert!(init_out.contains("Initialized APS package"));
    let add_out = run_apm_command(&["add".to_string(), "aps-http".to_string()], &tmp_pkg).unwrap();
    assert!(add_out.contains("aps-http"));
    let lock_txt = fs::read_to_string(tmp_pkg.join("aps.lock")).unwrap();
    assert!(lock_txt.contains("checksum = "));
    let _ = PackageManifest::compute_checksum("aps-http", "1.2.0");
}

#[test]
fn test_ama_safety_13_method_call_conflicting_borrow_rejected_e0302() {
    let src = r#"
fn main() {
    var v = Vec.new()
    v.push(write v)
}
"#;
    let res = check_code(src);
    assert!(res.diagnostics.has_errors());
    assert!(res.diagnostics.diagnostics.iter().any(|d| d.code == "E0302"));
}

#[test]
fn test_runtime_vec_dynamic_growth_and_pop() {
    let src = r#"
fn main() {
    var v = Vec.new()
    for i in 0..100 {
        v.push(i * 10)
    }
    assert_eq(v.len(), 100)
    assert_eq(v[0], 0)
    assert_eq(v[99], 990)
    let last = v.pop()
    assert_eq(last, 990)
    assert_eq(v.len(), 99)
    println("VEC_EXPANSION_OK")
}
"#;
    let out_dir = PathBuf::from("target/test_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe_path = out_dir.join(if cfg!(windows) {
        "vec_test.exe"
    } else {
        "vec_test"
    });
    let pipeline = CompilerPipeline::new(3);
    pipeline
        .build_executable("vec_test.aps", src, &exe_path)
        .expect("should compile vector dynamic growth test");
    let output = Command::new(&exe_path)
        .output()
        .expect("should execute vec binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("VEC_EXPANSION_OK"));
}

#[test]
fn test_runtime_divide_by_zero_panic_protection() {
    let src = r#"
fn main() {
    let zero: i64 = 0
    let res = 100 / zero
    println("SHOULD_NOT_REACH_HERE")
}
"#;
    let out_dir = PathBuf::from("target/test_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe_path = out_dir.join(if cfg!(windows) {
        "div_zero.exe"
    } else {
        "div_zero"
    });
    let pipeline = CompilerPipeline::new(0);
    pipeline
        .build_executable("div_zero.aps", src, &exe_path)
        .expect("should compile div by zero binary");
    let output = Command::new(&exe_path)
        .output()
        .expect("should execute div zero binary");
    // Should exit with panic status 101, not a segfault/hardware fault
    assert_eq!(output.status.code(), Some(101));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("attempt to divide by zero"));
}

#[test]
fn test_traits_and_contract_enforcement() {
    let invalid_src = r#"
trait Describable {
    fn describe(self: read Self) -> String
}

struct Item {
    id: i64
}

impl Describable for Item {
}

fn main() {
}
"#;
    let res = check_code(invalid_src);
    assert!(res.diagnostics.has_errors());
    assert!(res.diagnostics.diagnostics.iter().any(|d| d.code == "E0110"));

    let valid_src = r#"
trait Describable {
    fn describe(self: read Self) -> String
}

struct Item {
    id: i64
}

impl Describable for Item {
    fn describe(self: read Item) -> String {
        return "item"
    }
}

fn main() {
    let it = Item { id: 42 }
    let desc = it.describe()
    println(desc)
}
"#;
    let valid_res = check_code(valid_src);
    assert!(!valid_res.diagnostics.has_errors());

    let out_dir = PathBuf::from("target/test_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe_path = out_dir.join(if cfg!(windows) {
        "trait_test.exe"
    } else {
        "trait_test"
    });
    let pipeline = CompilerPipeline::new(2);
    pipeline
        .build_executable("trait_test.aps", valid_src, &exe_path)
        .expect("should compile trait binary");
    let output = Command::new(&exe_path)
        .output()
        .expect("should execute trait binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("item"));
}

#[test]
fn test_extreme_performance_builtins_native_execution() {
    let src = r#"
fn main() {
    let arr: [i64; 4] = [100, 200, 300, 400]
    let s = simd_sum_i64(arr, 4)
    let mn = simd_min_i64(arr, 4)
    let mx = simd_max_i64(arr, 4)
    assert(s == 1000)
    assert(mn == 100)
    assert(mx == 400)

    let txt = "hello world\n"
    assert(simd_is_ascii(txt, 12))
    assert(simd_count_byte(txt, 12, 10) == 1)
    assert(simd_find_byte(txt, 12, 32) == 5)

    let part = str_slice("benchmark_turbo", 10, 5)
    assert(part == "turbo")
    assert(str_find("benchmark_turbo", "turbo") == 10)
    assert(str_find_byte("benchmark_turbo", 95) == 9)
    assert(str_view_eq(part, 5, "turbo", 5))

    if likely(s == 1000) {
        let resp = http_response_fast(200, "text/plain", "OK")
        assert(str_find(resp, "200 OK") >= 0)
    }
    println("PERF_OK")
}
"#;
    let res = check_code(src);
    assert!(!res.diagnostics.has_errors());

    let out_dir = PathBuf::from("target/test_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe_path = out_dir.join(if cfg!(windows) {
        "extreme_perf_test.exe"
    } else {
        "extreme_perf_test"
    });
    let pipeline = CompilerPipeline::new(2);
    pipeline
        .build_executable("extreme_perf_test.aps", src, &exe_path)
        .expect("should compile extreme perf binary");
    let output = Command::new(&exe_path)
        .output()
        .expect("should execute extreme perf binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PERF_OK"));
}

#[test]
fn test_architectural_solutions_region_promotion_and_safety() {
    // 1. Direct region escape is rejected by E0303 (Lifetime Lattice check)
    let bad_src = r#"
struct User {
    id: i64
}

fn test_escape() -> i64 {
    var global_user: User = User { id: 0 }
    region request_arena {
        let u = request_arena.alloc(User { id: 42 })
        global_user = u
    }
    return global_user.id
}
"#;
    let bad_res = check_code(bad_src);
    assert!(bad_res.diagnostics.has_errors());
    assert!(bad_res.diagnostics.diagnostics.iter().any(|d| d.code == "E0303"));

    // 2. Safe explicit promotion out of region succeeds
    let good_src = r#"
struct User {
    id: i64
}

fn test_promote() -> i64 {
    var global_user: User = User { id: 0 }
    region request_arena {
        region_set_quota(request_arena, 1048576)
        let u = request_arena.alloc(User { id: 42 })
        global_user = promote(u)
    }
    return global_user.id
}

fn main() {
    let uid = test_promote()
    assert(uid == 42)
    println("ARCH_SOLUTIONS_OK")
}
"#;
    let good_res = check_code(good_src);
    assert!(!good_res.diagnostics.has_errors());

    let out_dir = PathBuf::from("target/test_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe_path = out_dir.join(if cfg!(windows) {
        "arch_solutions_test.exe"
    } else {
        "arch_solutions_test"
    });
    let pipeline = CompilerPipeline::new(2);
    pipeline
        .build_executable("arch_solutions_test.aps", good_src, &exe_path)
        .expect("should compile architectural solutions binary");
    let output = Command::new(&exe_path)
        .output()
        .expect("should execute architectural solutions binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ARCH_SOLUTIONS_OK"));
}

#[test]
fn test_hashmap_numeric_keys_and_string_security_fixes() {
    let src = r#"
module test_fixes

import std.collections

fn main() {
    // 1. Verify numeric keys >= 65536 in HashMap (previously crashed with 0xC0000005)
    var m = HashMap<i64, i64>.new()
    m.insert(100000, 42)
    m.insert(100008, 99)
    match m.get(100000) {
        Some(v) => assert(v == 42),
        None => panic("missing key 100000")
    }
    match m.get(100008) {
        Some(v) => assert(v == 99),
        None => panic("missing key 100008")
    }

    // 2. Verify string parsing on edge cases
    let s = "   -123456"
    assert(s.to_i64() == -123456)

    // 3. Verify sha256_hex
    let hash = sha256_hex("aps-security-audit")
    assert(hash.len() == 64)

    println("FIXES_VERIFIED_OK")
}
"#;
    let res = check_code(src);
    assert!(!res.diagnostics.has_errors());

    let out_dir = PathBuf::from("target/test_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe_path = out_dir.join(if cfg!(windows) {
        "security_fixes_test.exe"
    } else {
        "security_fixes_test"
    });
    let pipeline = CompilerPipeline::new(2);
    pipeline
        .build_executable("security_fixes_test.aps", src, &exe_path)
        .expect("should compile security fixes test binary");
    let output = Command::new(&exe_path)
        .output()
        .expect("should execute security fixes test binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FIXES_VERIFIED_OK"));
}
