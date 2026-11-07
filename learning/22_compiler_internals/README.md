# Глава 22: Внутреннее устройство компилятора APS (Compiler Internals)

## 1. Полный конвейер компиляции APS

```text
APS Source (*.aps)
       ↓
  1. Lexer (compiler/lexer/)         --> Поток токенов с координатами Span
       ↓
  2. Parser (compiler/parser/)       --> Абстрактное синтаксическое дерево (AST)
       ↓
  3. Semantic (compiler/semantic/)   --> Таблицы символов, проверка областей видимости
       ↓
  4. Type Checker (compiler/types/)  --> Вывод и унификация типов
       ↓
  5. AMA Engine (compiler/ama/)      --> Граф владения OLG, проверка read/write/take, Escape Analysis, CTFP
       ↓
  6. HIR (compiler/hir/)             --> Десахаризация (циклы for, методы impl)
       ↓
  7. MIR (compiler/mir/)             --> Граф потока управления CFG с точками Drop
       ↓
  8. APS IR & Opt (compiler/ir/)     --> Платформенно-независимые оптимизации (Constant Folding, Inlining)
       ↓
  9. LLVM Backend (compiler/codegen/)--> Генерация типизированного LLVM IR (*.ll)
       ↓
 10. Linker & Runtime (runtime/)     --> Нативный исполняемый файл (*.exe / ELF)
```

## 2. Разбор ключевых модулей компилятора
- `compiler/lexer/mod.rs`: посимвольный разбор UTF-8, поддержка префиксов `0x`, `0b`, `0o`, строковых литералов с экранированием.
- `compiler/parser/mod.rs`: алгоритм рекурсивного спуска с операторным приоритетом (Pratt Parsing) для бинарных и унарных выражений.
- `compiler/ama/mod.rs`: построение карты активных заимствований `active_borrows`, проверка непересекающихся путей полей структуры (Field-Disjoint Projection) и предотвращение выхода ссылок за пределы `region`.
- `compiler/optimize/mod.rs`: свёртка константных выражений, удаление мертвого кода и встраивание (inlining) легковесных чистых функций.
- `compiler/codegen/mod.rs`: генерация строгого SSA-представления LLVM IR и связывание с рантаймом `runtime/aps_runtime.c`.

## 3. Практическое руководство: как добавить новую конструкцию в язык APS
1. Добавить новый токен в `compiler/lexer/mod.rs`.
2. Расширить перечисление `ExprKind` или `Stmt` в `compiler/ast/mod.rs`.
3. Добавить ветку разбора в `compiler/parser/mod.rs`.
4. Реализовать проверку типов в `compiler/semantic/mod.rs` и правила AMA в `compiler/ama/mod.rs`.
5. Реализовать генерацию кода в `compiler/codegen/mod.rs`.
6. Добавить тесты в `tests/`.
