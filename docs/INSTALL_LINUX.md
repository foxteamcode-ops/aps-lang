# Инструкция по установке и сборке APS на Linux (`x86_64` / `ARM64`)

## 1. Системные требования

- **ОС:** Ubuntu 22.04+ / Debian 12+ / Fedora 39+ / Arch Linux (`x86_64-unknown-linux-gnu` или `aarch64-unknown-linux-gnu`)
- **Пакеты:** `clang` (LLVM 17+), `lld`, `build-essential`, `curl`, `git`
- **Rust Toolchain:** `rustup` (stable)

### Установка зависимостей (Ubuntu / Debian):
```bash
sudo apt update
sudo apt install -y build-essential clang lld llvm
```

## 2. Сборка компилятора APS из исходного кода

```bash
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

## 3. Проверка работы

```bash
aps version
aps test examples/specification
aps test std
aps run examples/specification/01_hello_world.aps
```
