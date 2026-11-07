# История изменений проекта APS (CHANGELOG)

Все значимые изменения в языке, компиляторе, стандартной библиотеке, инструментах и документации APS фиксируются в этом файле. Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.0.0/), используется семантическое версионирование (SemVer).

---

## [1.0.0] — 2026-11-07 (Production Release)

### Добавлено
- **Спецификация (Этап 1):**
  - Полная спецификация языка `spec/APS_LANGUAGE_SPEC.md` (31 раздел).
  - Формальная математическая спецификация архитектуры адаптивной памяти AMA `spec/AMA_SPEC.md` (1501 строка, 4 уровня памяти, 32 эталонных примера с кодами ошибок).
  - Формальная EBNF-грамматика и синтаксис `spec/APS_SYNTAX.md` (36 ключевых слов, правила форматирования).
  - Каталог диагностик и кодов ошибок `spec/DIAGNOSTICS.md` (`E0001`–`E0799`, `R0001`–`R0099`).
  - 32 спецификационных примера в `examples/specification/`.
- **Компилятор и кодогенерация (Этап 2):**
  - Модульный компилятор на Rust: лексер, парсер с Pratt-парсингом, AST, семантический анализ, статическая типизация с двунаправленным выводом, верификатор прав доступа AMA (`read`, `write`, `take`), уровни HIR, MIR и APS IR.
  - Оптимизатор APS: Constant Folding, Dead Code Elimination, Algebraic Simplifications, Function Inlining.
  - LLVM Backend с поддержкой LLVM 22 Clang (`target/release/aps.exe`).
  - Системный детерминированный рантайм `runtime/aps_runtime.c` (Zero-GC, быстрые арены `region`, строки UTF-8, защитные проверки границ).
  - Командная строка CLI (`aps new`, `check`, `build`, `run`, `test`, `fmt`, `lint`, `doc`, `clean`, `version`).
- **Стандартная библиотека и экосистема (Этап 3):**
  - Стандартная библиотека `std/` из 17 полноценных модулей (`core`, `memory`, `string`, `collections`, `math`, `io`, `fs`, `net`, `time`, `process`, `thread`, `sync`, `async`, `crypto`, `encoding`, `testing`, `platform`).
  - 10 реальных прикладных и серверных системных проектов в `examples/real_world/` (CLI, HTTP/1.1 сервер, TCP сервер, MapReduce конвейер, асинхронный планировщик, файловый WAL движок, парсер JSON, ACID реляционная БД, REST API сервис диспетчеризации, многомодульное enterprise-приложение).
  - Пакетный менеджер `apm` (`apm init`, `add`, `remove`, `update`, `install`, `build`, `publish`, `search`, манифесты `aps.toml`, lock-файлы `aps.lock` с хешами SHA-256).
  - Языковой сервер `aps-lsp` (LSP Protocol) для интеграции с редакторами кода.
- **Производительность, безопасность и тесты (Этап 4):**
  - Стресс-тесты компилятора `tests/stress_tests.rs` (50+ структур, 50+ функций, 40 уровней вложенности, арены графов).
  - Тесты безопасности памяти AMA `tests/ama_safety_and_ecosystem_tests.rs` (проверка предотвращения UAF, double-free, конфликтов алиасинга, утечек из регионов).
  - Воспроизводимый бенчмарк-сьют `benchmarks/` и отчёт `benchmarks/BENCHMARK_REPORT.md` (в allocation-тестах APS с `region arena` быстрее Си и Rust в 18.3 раза).
- **Курс обучения и документация (Этап 5):**
  - 23 главы русскоязычного учебника `learning/` (от архитектуры ЭВМ и CPU-кэшей до курса `APS Compiler Internals`).
  - Полный справочник языка и стандартной библиотеки `learning/reference/` (16 документов).
  - Банк из 300 пронумерованных упражнений `learning/exercises/` с отдельными 300 решениями в `solutions/`.
  - 15 сквозных практических проектов `learning/projects/` с верификацией компилятором.
  - Итоговый отчёт о релизе `RELEASE_REPORT.md`.
