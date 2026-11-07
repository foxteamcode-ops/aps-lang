# Инструкция по установке и сборке APS на Windows (`x86_64` / `ARM64`)

## 1. Системные требования

- **ОС:** Windows 10 / Windows 11 (x86-64 или ARM64)
- **Rust Toolchain:** `rustc` и `cargo` 1.80+ (`x86_64-pc-windows-msvc`)
- **LLVM Toolchain:** LLVM 18+ (`clang.exe`, рекомендуется LLVM 22, путь по умолчанию `C:\Program Files\LLVM\bin\clang.exe`)
- **Windows SDK & MSVC Build Tools:** Visual Studio 2022 / Build Tools с компонентом Desktop development with C++

## 2. Сборка компилятора, пакетного менеджера и LSP-сервера

Откройте PowerShell в корневом каталоге проекта `d:\APS\APS LANG`:

```powershell
cargo build --release
```

После сборки в каталоге `target\release\` будут созданы три нативных исполняемых файла:
- `aps.exe` — компилятор и CLI языка APS
- `apm.exe` — пакетный менеджер APS
- `aps-lsp.exe` — сервер языкового протокола (LSP) для редакторов кода (VS Code и др.)

## 3. Настройка переменной окружения `PATH`

Чтобы вызывать `aps` и `apm` из любой папки:

```powershell
$env:PATH += ";D:\APS\APS LANG\target\release;C:\Program Files\LLVM\bin"
```

## 4. Проверка установки и запуск тестовой программы

```powershell
aps version
aps test examples/specification
aps run examples/specification/01_hello_world.aps
```
