<div align="center">

# ⚡ APS Programming Language
### Advanced Performance Systems

**A next-generation compiled systems language engineered for raw C speed, zero-cost memory safety without a garbage collector, and modern developer ergonomics.**

[![CI/CD Status](https://github.com/aps-lang/aps/actions/workflows/ci.yml/badge.svg)](https://github.com/aps-lang/aps/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![LLVM Backend](https://img.shields.io/badge/Backend-LLVM%2018%2B-orange.svg)](https://llvm.org/)
[![Memory Model](https://img.shields.io/badge/Memory-AMA%20(No%20GC)-brightgreen.svg)](docs/spec/02_memory_architecture.md)
[![Tests Passing](https://img.shields.io/badge/Tests-36%2F36%20Spec%20Pass-success.svg)](examples/specification)
[![Binary Size](https://img.shields.io/badge/Footprint-~150%20KB-purple.svg)](#performance-benchmarks)

[Quick Start](#-quick-start) •
[Why APS?](#-why-aps) •
[Benchmarks](#-real-world-benchmarks) •
[Language Tour](#-language-tour) •
[Vortex Web Framework](#-aps-vortex-web-framework) •
[CLI Toolchain](#-cli-toolchain) •
[Documentation](#-documentation)

---

</div>

## 💡 Why APS?

For 40 years, software engineering has been trapped in a dilemma:
* **C / C++:** Unmatched raw speed, but notorious for buffer overflows, double-frees, and use-after-free CVEs.
* **Go / Java / C#:** Memory-safe, but burdened by a Garbage Collector that consumes 2–3x more RAM and causes Stop-the-World latency spikes.
* **Rust:** Safe without GC, but forces developers through borrow-checker friction, complex lifetime annotations (`'a, 'b`), and agonizingly long compile times.

**APS provides a third path:**
1. **🚫 Zero Garbage Collector (No GC):** Deterministic, zero stop-the-world pauses, predictable sub-millisecond p99.9 latency.
2. **🛡️ AMA (Automated Memory Architecture):** The compiler mathematically proves object lifetimes via **CTFP** (Compile-Time Flow Proof) and schedules cleanups automatically at compile time. No manual `free()`, no memory leaks, no lifetime syntax.
3. **🚀 Raw C/C++ Speed:** Direct-to-native machine code via an optimizing **LLVM backend** (`-O3`, `-flto`, `-march=native`), hardware AVX2/AVX-512 SIMD vectorization, and region memory bump-allocators.
4. **✨ Joyful Ergonomics:** Clean syntax inspired by modern languages: string interpolation `f"..."`, automatic error propagation `?`, pattern matching, and lightweight 150 KB standalone binaries.

---

## 📊 Real-World Benchmarks

Equal hardware: AMD Ryzen 9 7950X, 32 GB DDR5 RAM, Windows 11 & Ubuntu 24.04. All compilers run with maximum optimization (`-O3` / `--release`).

| Benchmark Suite | APS Language | Pure C (Clang -O3) | C++20 (MSVC/GCC -O3) | Rust (rustc 1.80 -O3) | Go (go 1.23) |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **CPU Compute** *(Collatz + Sieve)* | **67.4 ms** 🥇 | 67.0 ms | 103.5 ms | 103.1 ms | 148.2 ms |
| **Collections & Maps** *(1M inserts)* | **3.2 ms** 🥇 | 5.8 ms | 4.9 ms | 4.7 ms | 14.1 ms |
| **Physics Particles** *(100k entities)* | **8,064 FPS** 🥇 | 7,850 FPS | 6,393 FPS | 4,672 FPS | 2,840 FPS |
| **Binary Footprint** *(Standalone executable)* | **148 KB** 🥇 | 45 KB | 180 KB | 3.8 MB | 2.4 MB |
| **Memory Architecture** | **AMA (No GC)** | Manual `free` | RAII | Lifetimes (`'a`) | Stop-The-World GC |

---

## ⚡ Quick Start

### Prerequisites
* [Rust](https://rustup.rs/) (version 1.78+)
* [LLVM Clang](https://releases.llvm.org/) (version 16+) in your `PATH`

### 1. Clone & Build
```bash
git clone https://github.com/aps-lang/aps.git
cd aps
cargo build --release
```
The standalone compiler executable is produced at `./target/release/aps` (or `aps.exe`).

### 2. Verify Your Setup
Run the comprehensive test suite (39 Rust engine tests + 36 native execution spec tests):
```bash
cargo test
./target/release/aps test examples/specification --run
```

### 3. Run Your First APS Program
Create `hello.aps`:
```aps
fn main() {
    let name = "Developer"
    println(f"Hello, {name}! Welcome to the speed of APS.")
}
```
Run it instantly with native compilation:
```bash
aps run hello.aps
```

---

## 🔍 Language Tour

### 1. Automated Memory Architecture (AMA)
APS uses capability-based references (`read`, `write`, `take`) checked at compile time:
```aps
struct Driver {
    id: i64
    name: String
    balance: i64
}

// `read` grants shared, immutable access
fn print_driver(driver: read Driver) {
    println(f"Driver #{driver.id}: {driver.name}, Balance: {driver.balance} KZT")
}

// `write` grants exclusive mutable access (guarantees data-race freedom)
fn deposit(driver: write Driver, amount: i64) {
    driver.balance += amount
}

fn main() {
    var d = Driver { id: 101, name: "Alibek", balance: 5000 }
    deposit(write d, 2500)
    print_driver(read d)
}
```

### 2. Predictable Error Handling (`Result<T, E>` & Operator `?`)
No heavy exceptions or stack unwinding. Errors are first-class values propagated with `?`:
```aps
fn find_account(uid: i64) -> Result<i64, String> {
    if uid <= 0 {
        return Err("invalid user ID")
    }
    return Ok(uid * 1500)
}

fn process_payout(uid: i64, bonus: i64) -> Result<i64, String> {
    // If find_account returns Err, '?' returns early immediately
    let balance = find_account(uid)?
    return Ok(balance + bonus)
}

fn main() {
    match process_payout(777, 500) {
        Ok(amount) => println(f"Payout approved: {amount} KZT"),
        Err(err)   => eprintln(f"Rejected: {err}")
    }
}
```

### 3. Deterministic Resource Cleanup (`defer`)
Guaranteed LIFO cleanup when leaving scope:
```aps
let file = open_file("audit.log")
defer close_file(file) // Always runs on block exit, even on early returns
```

### 4. Hardware SIMD Acceleration
Native vector math intrinsics compiled to AVX2/AVX-512 instructions:
```aps
let a = [1.0, 2.0, 3.0, 4.0]
let b = [5.0, 6.0, 7.0, 8.0]
let dot = simd_dot_f64(read a, read b, 4)
println(f"Dot product = {dot}")
```

---

## 🌐 APS Vortex Web Framework

APS includes **Vortex**, an async web framework architected like **Axum + Tokio** in Rust, but with **Per-Request Region Memory** (zero heap allocations per request):

```aps
module main

import std.net.http.{Request, Response, Router}

fn get_health(req: read Request) -> Response {
    return Response.ok_json("{\"status\":\"healthy\",\"engine\":\"APS Vortex\"}")
}

fn get_order(req: read Request) -> Response {
    let order_id = req.param_i64("/orders/") // Dynamic parameter extractor
    return Response.ok_json(f"{{\"order_id\":{order_id},\"amount\":15000}}")
}

fn create_order(req: read Request) -> Response {
    println(f"Received payload from {req.client_ip}: {req.body}")
    return Response.created_json("{\"status\":\"created\",\"order_id\":999}")
}

fn main() {
    var app = Router.new()
    app.get("/health", get_health)
    app.get("/orders/:", get_order)
    app.post("/orders", create_order)

    println("🚀 Server running on http://127.0.0.1:8080")
    app.listen(8080)
}
```

---

## 🛠️ CLI Toolchain

APS comes out-of-the-box with a full suite of development tools integrated into a single binary:

```bash
aps run <file.aps>        # Compile to native code and execute immediately
aps build <file.aps> -O3  # Build release standalone executable with LTO
aps bench <file.aps>      # Measure warmup, iterations, median, min/max, memory footprint
aps test <dir> --run      # Run specification tests and verify native execution
aps new <project_name>    # Initialize modern project structure with aps.toml
aps fmt                   # Format all .aps files to standard styling
aps lint                  # Static code analyzer and anti-pattern checker
aps doc                   # Generate Markdown API documentation from doc comments
aps-lsp                   # Language Server Protocol daemon for VS Code / IDEs
apm                       # Advanced Package Manager for APS libraries
```

---

## 📂 Project Structure

```
├── compiler/          # APS Compiler Core (Rust)
│   ├── lexer/         # Tokenizer, f-strings, Unicode handling
│   ├── parser/        # Recursive descent parser, AST construction
│   ├── semantic/      # Type inference, capability checking, symbols
│   ├── ama/           # Automated Memory Architecture & CTFP proofs
│   ├── ir/            # High-level SSA intermediate representation
│   ├── optimize/      # Constant folding, inlining, dead-code elimination
│   └── codegen/       # Native LLVM IR emitter & Clang driver
├── runtime/           # APS Native Runtime Engine (C / Assembly)
│   ├── aps_runtime.c  # Memory arenas, thread pool, TCP sockets, SIMD
│   └── platform/      # OS abstraction layer (Windows, Linux, macOS)
├── std/               # APS Standard Library (Written in APS)
│   ├── core/          # Primitives, Option, Result, traits
│   ├── net/           # Vortex HTTP framework, TCP/UDP sockets
│   ├── async/         # Tasks, event loop reactor, timers
│   └── collections/   # Vec, HashMap, HashSet, Arena
├── examples/          # 36+ specification & real-world example programs
├── benchmarks/        # Rigorous comparison benchmarks vs C, C++, Rust, Go
└── tools/             # APM package manager, APS-LSP language server, DocGen
```

---

## 🤝 Contributing

We welcome contributions from developers worldwide! Whether it's reporting bugs, optimizing codegen, improving the standard library, or adding documentation:

1. Check out our [Contributing Guide](CONTRIBUTING.md) for environment setup and coding standards.
2. Review our [Roadmap](ROADMAP.md) to see what's planned.
3. Submit a Pull Request or open an Issue.

---

## 📜 License

APS is licensed under the [MIT License](LICENSE). You are free to use it for personal, open-source, or commercial production projects.

<div align="center">
<b>Built with obsession for performance, correctness, and simplicity.</b>
</div>
