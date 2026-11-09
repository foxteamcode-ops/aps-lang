use aps::compiler::CompilerPipeline;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_stress_many_functions_and_structs() {
    let mut code = String::new();
    code.push_str("module stress_test\n\n");

    // Generate 50 distinct structs
    for i in 0..50 {
        code.push_str(&format!(
            "struct Struct{} {{\n    id: i64\n    val: i64\n}}\n\n",
            i
        ));
    }

    // Generate 50 distinct functions
    for i in 0..50 {
        code.push_str(&format!(
            "fn compute_{}(s: read Struct{}) -> i64 {{\n    return s.id + s.val + {}\n}}\n\n",
            i, i, i
        ));
    }

    code.push_str("fn main() {\n    var total: i64 = 0\n");
    for i in 0..50 {
        code.push_str(&format!(
            "    let s{} = Struct{} {{ id: {}, val: {} }}\n    total += compute_{}(read s{})\n",
            i, i, i, i * 2, i, i
        ));
    }
    code.push_str("    println(total)\n}\n");

    let pipeline = CompilerPipeline::new(2);
    let res = pipeline.check_source("stress_gen.aps", &code);
    assert!(!res.diagnostics.has_errors());
    assert_eq!(res.program.items.len(), 101); // 50 structs + 50 fns + main
}

#[test]
fn test_stress_deeply_nested_expressions_and_blocks() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let mut code = String::new();
            code.push_str("fn main() {\n    var x: i64 = 0\n");

            // 40 levels of nested if-else blocks
            for i in 0..40 {
                code.push_str(&format!("{}if x >= 0 {{\n", "    ".repeat(i + 1)));
                code.push_str(&format!("{}x += 1\n", "    ".repeat(i + 2)));
            }
            for i in (0..40).rev() {
                code.push_str(&format!("{}}}\n", "    ".repeat(i + 1)));
            }

            code.push_str("    println(x)\n}\n");

            let pipeline = CompilerPipeline::new(2);
            let res = pipeline.check_source("nested_stress.aps", &code);
            assert!(!res.diagnostics.has_errors());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn test_stress_region_memory_allocation_graph() {
    let src = r#"
struct Node {
    id: i64
    weight: i64
}

fn main() {
    var total_weight: i64 = 0
    region graph_arena {
        var idx: i64 = 0
        while idx < 1000 {
            let node = graph_arena.alloc(Node { id: idx, weight: idx * 3 })
            total_weight += node.weight
            idx += 1
        }
    }
    println(total_weight)
}
"#;
    let pipeline = CompilerPipeline::new(3);
    let res = pipeline.check_source("region_stress.aps", src);
    if res.diagnostics.has_errors() {
        eprintln!("{}", res.diagnostics.render_all(src));
    }
    assert!(!res.diagnostics.has_errors());
    assert!(res.ama_report.active_regions.contains(&"graph_arena".to_string()));

    let out_dir = PathBuf::from("target/stress_artifacts");
    let _ = fs::create_dir_all(&out_dir);
    let exe = out_dir.join(if cfg!(windows) { "region_stress.exe" } else { "region_stress" });
    pipeline.build_executable("region_stress.aps", src, &exe).expect("compile region stress");
}
