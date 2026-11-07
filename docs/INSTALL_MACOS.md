# Инструкция по установке и сборке APS на macOS (`ARM64 Apple Silicon` / `x86_64`)

## 1. Системные требования

- **ОС:** macOS 13 Ventura или новее (`aarch64-apple-darwin` или `x86_64-apple-darwin`)
- **Xcode Command Line Tools:** включают системный `clang` и компоновщик Mach-O
- **Rust Toolchain:** `rustup` (stable)

### Установка инструментов командной строки:
```bash
xcode-select --install
```

## 2. Сборка компилятора APS

```bash
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

## 3. Проверка и запуск

```bash
aps version
aps test examples/specification
aps test std
aps run examples/specification/01_hello_world.aps
```
