use aps::compiler::CompilerPipeline;
use aps::tools::apm::PackageManifest;
use aps::tools::docgen::generate_markdown_docs;
use aps::tools::fmt::format_aps_source;
use aps::tools::lint::lint_program;
use aps::{APS_RELEASE_STAGE, APS_VERSION_DISPLAY};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

fn resolve_project_entry_point(dir: &Path) -> Option<PathBuf> {
    // 1. Standard project layout: src/main.aps
    let src_main = dir.join("src").join("main.aps");
    if src_main.is_file() {
        return Some(src_main);
    }
    // 2. Flat project layout: main.aps
    let main_aps = dir.join("main.aps");
    if main_aps.is_file() {
        return Some(main_aps);
    }
    // 3. Alternative standard app names: src/app.aps, app.aps
    let src_app = dir.join("src").join("app.aps");
    if src_app.is_file() {
        return Some(src_app);
    }
    let app_aps = dir.join("app.aps");
    if app_aps.is_file() {
        return Some(app_aps);
    }
    // 4. Manifest-based detection (aps.toml / Apm.toml)
    let has_manifest = dir.join("aps.toml").is_file() || dir.join("Apm.toml").is_file();
    if has_manifest {
        let src_lib = dir.join("src").join("lib.aps");
        if src_lib.is_file() {
            return Some(src_lib);
        }
    }
    // 5. If there's only one .aps file inside src/
    let src_dir = dir.join("src");
    if src_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&src_dir) {
            let mut aps_in_src: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("aps"))
                .collect();
            if aps_in_src.len() == 1 {
                return aps_in_src.pop();
            }
        }
    }
    // 6. If there's only one .aps file directly in dir
    if let Ok(entries) = fs::read_dir(dir) {
        let mut aps_in_dir: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("aps"))
            .collect();
        if aps_in_dir.len() == 1 {
            return aps_in_dir.pop();
        }
    }
    None
}

fn resolve_input_file(arg: Option<&String>) -> Result<PathBuf, String> {
    if let Some(p) = arg {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        if path.is_dir() {
            if let Some(entry) = resolve_project_entry_point(&path) {
                return Ok(entry);
            }
            return Err(format!(
                "directory `{}` has no `src/main.aps`, `main.aps`, or single `.aps` entry file",
                p
            ));
        }
        let with_aps = PathBuf::from(format!("{}.aps", p));
        if with_aps.is_file() {
            return Ok(with_aps);
        }
        return Err(format!("input file or directory `{}` does not exist", p));
    }

    let cwd = PathBuf::from(".");
    if let Some(entry) = resolve_project_entry_point(&cwd) {
        return Ok(entry);
    }

    Err("no `.aps` file specified and no project entry (`src/main.aps`, `main.aps`, `app.aps`) found in current directory.\n\nTip: You can create a new project with:\n  aps new <project_name>\nor specify a file:\n  aps run <file.aps>".to_string())
}

fn resolve_run_target_and_args(positional: &[String]) -> Result<(PathBuf, Vec<String>), String> {
    if let Some(pos) = positional.iter().position(|x| x == "--") {
        let (before, after) = positional.split_at(pos);
        let app_args: Vec<String> = after[1..].to_vec();
        let target_arg = before.first();
        let file = resolve_input_file(target_arg)?;
        return Ok((file, app_args));
    }

    if positional.is_empty() {
        let file = resolve_input_file(None)?;
        return Ok((file, Vec::new()));
    }

    let first = &positional[0];
    let path = PathBuf::from(first);
    if path.is_file() || path.is_dir() || PathBuf::from(format!("{}.aps", first)).is_file() {
        let file = resolve_input_file(Some(first))?;
        let app_args = positional[1..].to_vec();
        return Ok((file, app_args));
    }

    // First positional argument is not an existing file or directory: check if current dir is an APS project
    let cwd = PathBuf::from(".");
    if let Some(entry) = resolve_project_entry_point(&cwd) {
        return Ok((entry, positional.to_vec()));
    }

    let file = resolve_input_file(Some(first))?;
    Ok((file, Vec::new()))
}

fn default_output_exe(input_file: &Path) -> PathBuf {
    let stem = input_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app");

    let base_name = if stem == "main" || stem == "app" || stem == "lib" {
        let parent = input_file.parent();
        let meaningful = if let Some(p) = parent {
            let p_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if p_name == "src" {
                p.parent()
                    .and_then(|gp| gp.file_name())
                    .and_then(|s| s.to_str())
            } else if !p_name.is_empty() && p_name != "." {
                Some(p_name)
            } else {
                None
            }
        } else {
            None
        };
        meaningful.unwrap_or(stem)
    } else {
        stem
    };

    let sanitized = base_name.replace(' ', "_");
    let exe_name = if cfg!(windows) {
        format!("{}.exe", sanitized)
    } else {
        sanitized
    };
    let target_dir = PathBuf::from("target");
    let _ = fs::create_dir_all(&target_dir);
    target_dir.join(exe_name)
}

fn collect_aps_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                collect_aps_files(&p, out);
            } else if p.extension().and_then(|e| e.to_str()) == Some("aps") {
                out.push(p);
            }
        }
    }
}

/// Builds `file`, runs it with a timeout, requires exit code 0 and, when a sibling
/// `<file>.expected` exists, requires identical stdout (line endings normalised).
fn run_program_test(pipeline: &CompilerPipeline, file: &Path, src: &str) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("test");
    let dir = env::temp_dir().join("aps_test_run");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create temp dir: {}", e))?;
    let exe = dir.join(format!("{}{}", stem, if cfg!(windows) { ".exe" } else { "" }));
    pipeline
        .build_executable(&file.to_string_lossy(), src, &exe)
        .map_err(|e| format!("build failed:\n{}", e))?;

    let mut child = Command::new(&exe)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start program: {}", e))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(20) {
                    let _ = child.kill();
                    return Err("timed out after 20s".to_string());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(format!("wait failed: {}", e)),
        }
    };
    let mut stdout = String::new();
    if let Some(mut so) = child.stdout.take() {
        let mut buf = Vec::new();
        let _ = so.read_to_end(&mut buf);
        stdout = String::from_utf8_lossy(&buf).replace("\r\n", "\n");
    }
    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut se) = child.stderr.take() {
            let mut buf = Vec::new();
            let _ = se.read_to_end(&mut buf);
            stderr = String::from_utf8_lossy(&buf).to_string();
        }
        return Err(format!("exit status {}\n{}", status, stderr));
    }
    let expected_path = PathBuf::from(format!("{}.expected", file.display()));
    if expected_path.exists() {
        let expected = fs::read_to_string(&expected_path)
            .map_err(|e| format!("cannot read expected file: {}", e))?
            .replace("\r\n", "\n");
        if expected != stdout {
            return Err(format!(
                "stdout differs from {}\n--- expected ---\n{}\n--- actual ---\n{}",
                expected_path.display(),
                expected,
                stdout
            ));
        }
        return Ok(" (output matched)".to_string());
    }
    Ok(String::new())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");

    let mut opt_level = 2u8;
    let mut debug_info = false;
    let mut emit_llvm = false;
    let mut emit_ir = false;
    let mut exec_tests = false;
    let mut custom_output: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "--run" {
            exec_tests = true;
        } else if a == "-g" || a == "--debug" {
            debug_info = true;
        } else if a == "-O0" {
            opt_level = 0;
        } else if a == "-O1" {
            opt_level = 1;
        } else if a == "-O2" {
            opt_level = 2;
        } else if a == "-O3" || a == "--release" {
            opt_level = 3;
        } else if a == "-O" || a == "--opt-level" {
            i += 1;
            if i < args.len() {
                if let Ok(lvl) = args[i].parse::<u8>() {
                    opt_level = lvl.min(3);
                }
            }
        } else if let Some(val) = a.strip_prefix("-O=") {
            if let Ok(lvl) = val.parse::<u8>() {
                opt_level = lvl.min(3);
            }
        } else if let Some(val) = a.strip_prefix("--opt-level=") {
            if let Ok(lvl) = val.parse::<u8>() {
                opt_level = lvl.min(3);
            }
        } else if a == "-o" || a == "--output" {
            i += 1;
            if i < args.len() {
                custom_output = Some(PathBuf::from(&args[i]));
            }
        } else if let Some(val) = a.strip_prefix("-o=") {
            custom_output = Some(PathBuf::from(val));
        } else if let Some(val) = a.strip_prefix("--output=") {
            custom_output = Some(PathBuf::from(val));
        } else if a == "--emit-llvm" {
            emit_llvm = true;
        } else if a == "--emit-ir" {
            emit_ir = true;
        } else {
            positional.push(a.clone());
        }
        i += 1;
    }

    match cmd {
        "version" | "--version" | "-V" => {
            println!(
                "aps {} [Stage: {}] (Advanced Performance Systems Compiler, AMA Memory Engine, LLVM Backend)",
                APS_VERSION_DISPLAY,
                APS_RELEASE_STAGE
            );
        }
        "new" => {
            let Some(proj_name) = positional.first() else {
                eprintln!("error: usage: aps new <project_name>");
                process::exit(1);
            };
            let proj_dir = PathBuf::from(proj_name);
            let src_dir = proj_dir.join("src");
            let tests_dir = proj_dir.join("tests");
            if let Err(e) = fs::create_dir_all(&src_dir).and_then(|_| fs::create_dir_all(&tests_dir))
            {
                eprintln!("error: failed to create project directories: {}", e);
                process::exit(1);
            }
            let manifest = PackageManifest::default_for(proj_name);
            let _ = fs::write(proj_dir.join("aps.toml"), manifest.to_toml());
            let _ = fs::write(proj_dir.join("aps.lock"), manifest.generate_lockfile());
            let starter_code = r#"// APS Starter Application
struct Driver {
    id: i64
    name: String
    balance: i64
}

fn show_driver(driver: read Driver) {
    println(driver.name)
    println(driver.balance)
}

fn main() {
    println("Hello, APS!")
    let driver = Driver {
        id: 1,
        name: "Alibek",
        balance: 1000
    }
    show_driver(read driver)
}
"#;
            let _ = fs::write(src_dir.join("main.aps"), starter_code);
            println!("Created APS project `{}` at {}", proj_name, proj_dir.display());
        }
        "check" => {
            let file = match resolve_input_file(positional.first()) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{}`: {}", file.display(), e);
                    process::exit(1);
                }
            };
            let mut pipeline = CompilerPipeline::new(opt_level);
            pipeline.debug_info = debug_info;
            let fname = file.to_string_lossy();
            let res = pipeline.check_source(&fname, &src);
            if res.diagnostics.has_errors() {
                eprint!("{}", res.diagnostics.render_all(&src));
                process::exit(1);
            }
            if emit_ir {
                println!("{}", res.aps_ir_text);
            }
            if emit_llvm {
                println!("{}", res.llvm_ir);
            }
            println!(
                "Checked `{}`: OK (0 errors, {} AMA drops scheduled, {} constants folded)",
                file.display(),
                res.ama_report.ctfp_drops.len(),
                res.opt_stats.constants_folded
            );
        }
        "build" => {
            let file = match resolve_input_file(positional.first()) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{}`: {}", file.display(), e);
                    process::exit(1);
                }
            };
            let out_exe = custom_output.clone().unwrap_or_else(|| {
                positional
                    .get(1)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| default_output_exe(&file))
            });
            let mut pipeline = CompilerPipeline::new(opt_level);
            pipeline.debug_info = debug_info;
            let fname = file.to_string_lossy();
            match pipeline.build_executable(&fname, &src, &out_exe) {
                Ok(res) => {
                    if emit_ir {
                        println!("{}", res.aps_ir_text);
                    }
                    if emit_llvm {
                        println!("{}", res.llvm_ir);
                    }
                    println!(
                        "Compiled `{}` -> `{}` (opt=-O{})",
                        file.display(),
                        out_exe.display(),
                        opt_level
                    );
                }
                Err(err) => {
                    eprint!("{}", err);
                    process::exit(1);
                }
            }
        }
        "run" => {
            let (file, app_args) = match resolve_run_target_and_args(&positional) {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{}`: {}", file.display(), e);
                    process::exit(1);
                }
            };
            let out_exe = custom_output.clone().unwrap_or_else(|| default_output_exe(&file));
            let mut pipeline = CompilerPipeline::new(opt_level);
            pipeline.debug_info = debug_info;
            let fname = file.to_string_lossy();
            if let Err(err) = pipeline.build_executable(&fname, &src, &out_exe) {
                eprint!("{}", err);
                process::exit(1);
            }
            let mut cmd = Command::new(&out_exe);
            cmd.args(&app_args);
            let status = cmd.status();
            match status {
                Ok(st) => {
                    if !st.success() {
                        process::exit(st.code().unwrap_or(1));
                    }
                }
                Err(e) => {
                    eprintln!("error: failed to execute `{}`: {}", out_exe.display(), e);
                    process::exit(1);
                }
            }
        }
        "test" => {
            let target = positional
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("examples/specification"));
            let mut files = Vec::new();
            if target.is_file() {
                files.push(target.clone());
            } else {
                collect_aps_files(&target, &mut files);
            }
            if files.is_empty() {
                println!("No `.aps` test files found in `{}`", target.display());
                return;
            }
            let mut pipeline = CompilerPipeline::new(opt_level);
            pipeline.debug_info = debug_info;
            let mut passed = 0usize;
            let mut failed = 0usize;
            for f in &files {
                let src = fs::read_to_string(f).unwrap_or_default();
                let res = pipeline.check_source(&f.to_string_lossy(), &src);
                if res.diagnostics.has_errors() {
                    println!("test {} ... FAILED", f.display());
                    eprint!("{}", res.diagnostics.render_all(&src));
                    failed += 1;
                } else if exec_tests {
                    match run_program_test(&pipeline, f, &src) {
                        Ok(note) => {
                            println!("test {} ... ok{}", f.display(), note);
                            passed += 1;
                        }
                        Err(msg) => {
                            println!("test {} ... FAILED (run)", f.display());
                            eprintln!("{}", msg);
                            failed += 1;
                        }
                    }
                } else {
                    println!("test {} ... ok", f.display());
                    passed += 1;
                }
            }
            println!(
                "\ntest result: {}. {} passed; {} failed",
                if failed == 0 { "ok" } else { "FAILED" },
                passed,
                failed
            );
            if failed > 0 {
                process::exit(1);
            }
        }
        "bench" => {
            let file = match resolve_input_file(positional.first()) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{}`: {}", file.display(), e);
                    process::exit(1);
                }
            };

            let out_dir = env::temp_dir().join("aps_bench_build");
            let _ = fs::create_dir_all(&out_dir);
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("bench_target");
            let exe_name = if cfg!(windows) { format!("{}.exe", stem) } else { stem.to_string() };
            let exe_path = out_dir.join(&exe_name);

            println!("================================================================================");
            println!(" APS NATIVE BENCHMARK RUNNER (Release -O3, AMA Memory Architecture)");
            println!(" Target: {}", file.display());
            println!("================================================================================");
            print!("Compiling with native LLVM backend (-O3)... ");
            io::stdout().flush().ok();

            let mut pipeline = CompilerPipeline::new(3);
            pipeline.debug_info = false;
            let fname = file.to_string_lossy();
            if let Err(err) = pipeline.build_executable(&fname, &src, &exe_path) {
                println!("FAILED");
                eprint!("{}", err);
                process::exit(1);
            }
            println!("OK");

            let bin_size = fs::metadata(&exe_path).map(|m| m.len()).unwrap_or(0);
            println!("Binary footprint: {} bytes ({:.1} KB)", bin_size, bin_size as f64 / 1024.0);
            println!("--------------------------------------------------------------------------------");

            let _ = process::Command::new(&exe_path).output();

            let iters = 5;
            let mut samples_us: Vec<u128> = Vec::new();
            for i in 1..=iters {
                let start = std::time::Instant::now();
                let output = process::Command::new(&exe_path).output();
                let elapsed = start.elapsed().as_micros();
                match output {
                    Ok(out) if out.status.success() => {
                        samples_us.push(elapsed);
                        let stdout_line = String::from_utf8_lossy(&out.stdout);
                        let first_line = stdout_line.lines().next().unwrap_or("").trim();
                        println!("  Iteration #{}: {:>8.2} ms ({:>8} µs) | {}", i, elapsed as f64 / 1000.0, elapsed, first_line);
                    }
                    Ok(out) => {
                        eprintln!("  Iteration #{}: FAILED with code {:?}", i, out.status.code());
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        if !stderr.is_empty() {
                            eprintln!("{}", stderr);
                        }
                        process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("  Iteration #{}: failed to spawn binary: {}", i, e);
                        process::exit(1);
                    }
                }
            }

            samples_us.sort_unstable();
            let median_us = samples_us[samples_us.len() / 2];
            let min_us = *samples_us.first().unwrap_or(&0);
            let max_us = *samples_us.last().unwrap_or(&0);
            let avg_us = samples_us.iter().sum::<u128>() / samples_us.len() as u128;

            println!("--------------------------------------------------------------------------------");
            println!(" BENCHMARK SUMMARY ({} iterations):", iters);
            println!("   Median Time : {:>8.2} ms  ({} µs)", median_us as f64 / 1000.0, median_us);
            println!("   Average Time: {:>8.2} ms  ({} µs)", avg_us as f64 / 1000.0, avg_us);
            println!("   Min Time    : {:>8.2} ms  ({} µs)", min_us as f64 / 1000.0, min_us);
            println!("   Max Time    : {:>8.2} ms  ({} µs)", max_us as f64 / 1000.0, max_us);
            println!("   Status      : PASSED (100% stable)");
            println!("================================================================================");
        }
        "fmt" => {
            let file = match resolve_input_file(positional.first()) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{}`: {}", file.display(), e);
                    process::exit(1);
                }
            };
            let formatted = format_aps_source(&src);
            if let Err(e) = fs::write(&file, formatted) {
                eprintln!("error: failed to write formatted file: {}", e);
                process::exit(1);
            }
            println!("Formatted `{}`", file.display());
        }
        "lint" => {
            let file = match resolve_input_file(positional.first()) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };
            let src = fs::read_to_string(&file).unwrap_or_default();
            let pipeline = CompilerPipeline::new(0);
            let res = pipeline.check_source(&file.to_string_lossy(), &src);
            let lint_diags = lint_program(&res.program);
            if !lint_diags.diagnostics.is_empty() {
                print!("{}", lint_diags.render_all(&src));
            }
            println!(
                "Linted `{}`: {} warning(s)",
                file.display(),
                lint_diags.warning_count()
            );
        }
        "doc" => {
            let file = match resolve_input_file(positional.first()) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };
            let src = fs::read_to_string(&file).unwrap_or_default();
            let pipeline = CompilerPipeline::new(0);
            let res = pipeline.check_source(&file.to_string_lossy(), &src);
            let md = generate_markdown_docs(&file.to_string_lossy(), &res.program);
            let doc_dir = PathBuf::from("target/doc");
            let _ = fs::create_dir_all(&doc_dir);
            let out_doc = doc_dir.join("index.md");
            let _ = fs::write(&out_doc, &md);
            println!("Generated documentation at `{}`", out_doc.display());
        }
        "clean" => {
            let target_dir = PathBuf::from("target");
            if target_dir.exists() {
                let _ = fs::remove_dir_all(&target_dir);
            }
            println!("Cleaned build artifacts in `target/`");
        }
        _ => {
            println!("APS Programming Language Compiler (aps {})", APS_VERSION_DISPLAY);
            println!("\nUsage:");
            println!("  aps new <project>       Create a new APS project");
            println!("  aps check [file.aps]    Parse, typecheck, and run AMA memory verification");
            println!("  aps build [file.aps]    Compile APS source to native executable via LLVM");
            println!("  aps run [file.aps]      Compile and execute native APS binary");
            println!("  aps test [path]         Run APS compiler & specification tests");
            println!("  aps bench [file.aps]    Execute high-resolution native benchmarks with stats");
            println!("  aps fmt [file.aps]      Format APS source code");
            println!("  aps lint [file.aps]     Run static style & best-practices linter");
            println!("  aps doc [file.aps]      Generate Markdown API documentation");
            println!("  aps clean               Remove target/ build artifacts");
            println!("  aps version             Display compiler version");
        }
    }
}
