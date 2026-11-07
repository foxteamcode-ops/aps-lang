# Глава 01: Установка компилятора, тулчейна и настройка окружения

## 1. Системные требования
APS работает на трёх основных платформах:
- **Windows:** 64-bit (x86-64 / ARM64), установлен LLVM Clang 18+ и MSVC Tools.
- **Linux:** Ubuntu 22+, Debian 12+, Fedora 39+, Arch Linux (`clang`, `build-essential`).
- **macOS:** Apple Silicon (M1/M2/M3/M4) или Intel x86-64, Xcode Command Line Tools.

## 2. Установка и сборка компилятора APS
В корне проекта выполните:
```bash
cargo build --release
```
Исполняемые файлы `aps` (компилятор), `apm` (пакетный менеджер) и `aps-lsp` (Language Server) будут созданы в папке `target/release`.

## 3. Добавление в системный PATH
### Windows (PowerShell):
```powershell
$env:PATH += ";D:\APS\APS LANG\target\release;C:\Program Files\LLVM\bin"
```
Также в корне проекта созданы удобные скрипты `aps.bat` и `apm.bat`.

## 4. Полный обзор команд интерфейса командной строки `aps`
1. `aps new <name>` — создать новый проект с манифестом `aps.toml` и исходником `src/main.aps`.
2. `aps check [file.aps]` — разобрать синтаксис, проверить типы и выполнить анализ безопасности памяти AMA.
3. `aps build [file.aps]` — скомпилировать программу через LLVM в нативный исполняемый файл `.exe` / ELF.
4. `aps run [file.aps]` — скомпилировать и немедленно выполнить программу.
5. `aps test [path]` — запустить тестирование набора программ или модулей.
6. `aps fmt [file.aps]` — автоматически отформатировать исходный код по стандарту APS.
7. `aps lint [file.aps]` — статический анализатор стиля и наименований.
8. `aps doc [file.aps]` — сгенерировать документацию API в формате Markdown.
9. `aps clean` — удалить временные артефакты компиляции из `target/`.
10. `aps version` — вывести текущую версию компилятора.
