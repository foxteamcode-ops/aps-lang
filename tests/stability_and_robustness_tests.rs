use aps::compiler::CompilerPipeline;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_fuzz_malformed_and_truncated_sources() {
    let pipeline = CompilerPipeline::new(2);

    let malformed_cases = vec![
        // 1. Unterminated string literal
        ("unterminated_str.aps", "fn main() { let s = \"unterminated string literal; }"),
        // 2. Unterminated block comment
        ("unterminated_comment.aps", "/* unclosed comment block\nfn main() { let x = 1 }\n"),
        // 3. Stray unicode and control characters
        ("stray_chars.aps", "fn main() { let \u{0000} \u{FFFF} = 10; }"),
        // 4. Truncated binary expression
        ("truncated_expr.aps", "fn main() { let x = 1 + ; }"),
        // 5. Deeply nested unclosed parentheses
        ("unclosed_parens.aps", "fn main() { let x = ((((((((((1)))))))) ; }"),
        // 6. Deeply unclosed braces
        ("unclosed_braces.aps", "fn main() { {{{{{{{{ let x = 1; }"),
        // 7. Completely empty source
        ("empty.aps", ""),
        // 8. Whitespace & comments only
        ("comments_only.aps", "   \n\t // single line comment\n /* multi \n line */ \n"),
        // 9. Struct without body
        ("broken_struct.aps", "struct Incomplete {"),
        // 10. Function without body or return type
        ("broken_fn.aps", "fn broken("),
    ];

    for (filename, src) in malformed_cases {
        // Robustness invariant: The compiler MUST NOT PANIC or crash (ICE).
        // It must handle broken input gracefully and produce diagnostics (or pass if empty).
        let res = pipeline.check_source(filename, src);
        if filename != "empty.aps" && filename != "comments_only.aps" {
            assert!(
                res.diagnostics.has_errors(),
                "Expected diagnostic errors for malformed input in `{}`",
                filename
            );
        }
    }
}

#[test]
fn test_fuzz_recursive_and_circular_types() {
    let pipeline = CompilerPipeline::new(2);

    // Circular struct definition A -> B -> A
    let circular_src = r#"
struct NodeA {
    b: NodeB
}
struct NodeB {
    a: NodeA
}

fn main() {
    let x: i64 = 42
    println(x)
}
"#;

    // Must not crash with stack overflow or infinite loop in layout/type resolution
    let res = pipeline.check_source("circular.aps", circular_src);
    // Should compile or diagnose gracefully without ICE
    let _ = res.llvm_ir;
}

#[test]
fn test_semantic_type_safety_torture() {
    let pipeline = CompilerPipeline::new(2);

    let type_torture_cases = vec![
        // 1. Calling undefined function
        (
            "undefined_fn.aps",
            "fn main() { undefined_func(1, 2, 3); }",
            "undefined function",
        ),
        // 2. Argument count mismatch
        (
            "arg_count_mismatch.aps",
            "fn add(a: i64, b: i64) -> i64 { return a + b; }\nfn main() { add(1, 2, 3); }",
            "argument",
        ),
        // 3. Mutating immutable variable
        (
            "immutable_mutation.aps",
            "fn main() { let x: i64 = 10; x = 20; }",
            "immutable",
        ),
        // 4. Using undeclared variable
        (
            "undeclared_var.aps",
            "fn main() { y = 50; }",
            "cannot find value",
        ),
    ];

    for (filename, src, expected_needle) in type_torture_cases {
        let res = pipeline.check_source(filename, src);
        assert!(
            res.diagnostics.has_errors(),
            "Expected semantic errors in `{}`",
            filename
        );
        let error_text = res.diagnostics.render_all(src);
        assert!(
            error_text.to_lowercase().contains(expected_needle),
            "Expected diagnostics in `{}` to mention `{}`. Got:\n{}",
            filename,
            expected_needle,
            error_text
        );
    }
}

#[test]
fn test_runtime_stress_recursion_and_dynamic_vectors() {
    let src = r#"
fn fibonacci(n: i64) -> i64 {
    if n <= 1 {
        return n
    }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

fn test_vector_growth(count: i64) -> i64 {
    var v = [0]
    for i in 1..count {
        v.push(i)
    }
    var sum: i64 = 0
    for i in 0..v.len() {
        sum += v[i]
    }
    return sum
}

fn main() {
    // 1. Recursive stress (Fibonacci 15 = 610)
    let fib15 = fibonacci(15)
    assert_eq(fib15, 610)

    // 2. Dynamic array reallocation stress (5,000 push operations)
    let v_sum = test_vector_growth(5000)
    // Sum of 0..4999 = (4999 * 5000) / 2 = 12497500
    assert_eq(v_sum, 12497500)

    println("SUCCESS_STABILITY_PASSED")
}
"#;

    let target_dir = PathBuf::from("target").join("test_robustness_bin");
    fs::create_dir_all(&target_dir).unwrap();
    let exe_path = target_dir.join("stability_test.exe");

    let pipeline = CompilerPipeline::new(2);
    pipeline
        .build_executable("stability_test.aps", src, &exe_path)
        .expect("should compile stability test binary");

    let output = Command::new(&exe_path)
        .output()
        .expect("Failed to execute stability test binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SUCCESS_STABILITY_PASSED"),
        "Test output did not contain success marker. Got: {}",
        stdout
    );

    let _ = fs::remove_file(exe_path);
}
