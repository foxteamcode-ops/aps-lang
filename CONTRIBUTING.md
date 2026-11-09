# Contributing to APS

Thank you for your interest in contributing to the **APS (Advanced Performance Systems)** programming language! We welcome contributions of all kinds: bug reports, compiler optimizations, standard library improvements, documentation, and language tooling.

---

## 🛠️ Development Setup

### Prerequisites
1. **Rust:** Stable toolchain (version 1.78 or higher). Install via [rustup](https://rustup.rs/):
   ```bash
   rustup default stable
   ```
2. **LLVM Clang:** Version 16 or newer with `clang` available in your system `PATH`.
   - **Windows:** Install via [LLVM GitHub Releases](https://github.com/llvm/llvm-project/releases) or Visual Studio Build Tools with Clang tools.
   - **Linux:** `sudo apt-get install clang llvm` (Ubuntu/Debian) or `sudo pacman -S clang llvm` (Arch).
   - **macOS:** `brew install llvm` or Xcode command line tools.

### Building from Source
```bash
git clone https://github.com/aps-lang/aps.git
cd aps
cargo build
```
For an optimized release build:
```bash
cargo build --release
```

---

## 🧪 Testing Guidelines

Before opening a pull request, you **must ensure all tests pass**:

### 1. Rust Compiler Unit & Integration Tests
Runs all internal compiler tests (lexer, parser, AMA safety proofs, optimizer, code generator):
```bash
cargo test
```
All 39 tests must pass with zero failures.

### 2. Specification Native Execution Tests
Runs the full suite of 36 specification programs, compiling each to native code through the LLVM backend and verifying execution output against expected values:
```bash
cargo run --bin aps -- test examples/specification --run
```
All 36 specification tests must pass.

### 3. Native Benchmark Smoke Test
Ensure zero-cost safety and optimization regressions have not occurred:
```bash
cargo run --bin aps -- bench benchmarks/01_cpu_collatz_primes.aps
```

---

## 📝 Commit Convention

We follow the [Conventional Commits](https://www.conventionalcommits.org/) standard. Each commit message should follow this format:

```
<type>(<scope>): <short description>

[optional body]
```

### Commit Types
- `feat`: A new user-facing language feature or compiler capability.
- `fix`: A bug fix in the compiler, runtime, or standard library.
- `perf`: A code change that improves compile-time or runtime performance.
- `docs`: Documentation updates (README, language spec, guides).
- `test`: Adding or modifying tests.
- `refactor`: Code change that neither fixes a bug nor adds a feature.
- `chore`: Tooling, CI, or dependency updates.

### Examples
- `feat(codegen): add error propagation operator '?' for Result and Option`
- `fix(parser): prevent ambiguous generic '<' comparison rollback`
- `perf(ama): elide stack alloca for void branch continuations`

---

## 🔀 Pull Request Process

1. **Fork the Repository:** Create your feature branch from `master`:
   ```bash
   git checkout -b feat/my-new-feature
   ```
2. **Implement Your Changes:** Keep changes focused and atomic.
3. **Write Tests:** Add unit tests in `tests/` or a specification test in `examples/specification/` if introducing syntax or runtime features.
4. **Run Format & Lint:**
   ```bash
   cargo fmt --check
   cargo clippy
   ```
5. **Open a PR:** Describe the motivation, problem solved, and link any relevant issues. Maintainers will review and assist with feedback.

---

## 📜 Architectural Invariants

When modifying compiler internals, always uphold these core invariants:
1. **Memory Safety Invariant:** Do not bypass AMA safety checks or allow unproven capability mutations.
2. **Deterministic Cleanup:** All allocations must have a deterministic lifecycle (stack, arena, or CTFP drop).
3. **Zero-Cost Abstractions:** High-level language constructs (`?`, `f"..."`, `match`) must compile down to lean, branch-predicted LLVM IR without hidden heap allocations.

Thank you for helping make APS the fastest and safest systems language in the world!
