# Спецификация системы ошибок и диагностик компилятора APS (`DIAGNOSTICS`)

**Версия спецификации:** 1.0 (Этап 1 — `v0.1.0-spec`)  
**Статус документа:** Нормативный стандарт диагностической подсистемы компилятора `aps` и языкового сервера `aps-lsp`  
**Связанные документы:** `DECISIONS.md`, `spec/APS_LANGUAGE_SPEC.md`, `spec/AMA_SPEC.md`, `spec/APS_SYNTAX.md`

---

## Оглавление

1. [Философия и принципы диагностической системы APS](#1-философия-и-принципы-диагностической-системы-aps)
2. [Архитектура диагностического движка (`aps_diagnostics`)](#2-архитектура-диагностического-движка-aps_diagnostics)
3. [Эталонный формат сообщения об ошибке и схема JSON-вывода](#3-эталонный-формат-сообщения-об-ошибке-и-схема-json-вывода)
4. [Категория 1: Синтаксические и лексические ошибки (`E0001`–`E0039`)](#4-категория-1-синтаксические-и-лексические-ошибки-e0001e0039)
5. [Категория 2: Ошибки прав доступа — Capability Errors (`E0040`–`E0099`)](#5-категория-2-ошибки-прав-доступа--capability-errors-e0040e0099)
6. [Категория 3: Ошибки типизации — Type Errors (`E0100`–`E0199`)](#6-категория-3-ошибки-типизации--type-errors-e0100e0199)
7. [Категория 4: Ошибки владения — Ownership Errors (`E0200`–`E0299`)](#7-категория-4-ошибки-владения--ownership-errors-e0200e0299)
8. [Категория 5: Безопасность памяти, регионы и времена жизни (`E0300`–`E0399`)](#8-категория-5-безопасность-памяти-регионы-и-времена-жизни-e0300e0399)
9. [Категория 6: Многопоточность и асинхронность (`E0400`–`E0499`)](#9-категория-6-многопоточность-и-асинхронность-e0400e0499)
10. [Категория 7: Модули, импорты и области видимости (`E0500`–`E0599`)](#10-категория-7-модули-импорты-и-области-видимости-e0500e0599)
11. [Категория 8: Безопасность FFI и блоков `unsafe` (`E0600`–`E0699`)](#11-категория-8-безопасность-ffi-и-блоков-unsafe-e0600e0699)
12. [Категория 9: Ошибки компоновки и кодогенерации — Linker Errors (`E0700`–`E0799`)](#12-категория-9-ошибки-компоновки-и-кодогенерации--linker-errors-e0700e0799)
13. [Категория 10: Ошибки времени выполнения и паники — Runtime Errors (`R0001`–`R0099`)](#13-категория-10-ошибки-времени-выполнения-и-паники--runtime-errors-r0001r0099)
14. [Предупреждения компилятора и линтера (`W0001`–`W0020`)](#14-предупреждения-компилятора-и-линтера-w0001w0020)

---

## 1. Философия и принципы диагностической системы APS

Сообщения компилятора об ошибках — это главный пользовательский интерфейс языка программирования. В языке APS диагностический движок проектируется как первоклассная подсистема, подчиняющаяся пяти инженерным правилам:

1. **Никаких загадочных однострочных сообщений:** компилятор никогда не выдаёт сообщения вида `syntax error` или `type mismatch`. Каждая диагностика обязана показать **что** произошло, **где** это произошло (с визуальной привязкой к исходному коду), **почему** это нарушает правила языка и **как** это исправить (`= help:`).
2. **Стабильные канонические коды ошибок:** каждая ошибка компиляции имеет уникальный неизменный код формата `E0xxx` (от `E0001` до `E0799`), каждая ошибка времени выполнения — `R0xxx` (`R0001`–`R0099`), каждое предупреждение — `W0xxx` (`W0001`–`W0099`). Команда `aps explain E0042` выводит подробную встроенную справку по коду.
3. **Подавление каскадных ложных срабатываний (Poisoning / Error Type Propagation):** если переменная `x` не смогла пройти проверку типов из-за ошибки в её инициализаторе, ей присваивается внутренний тип `Type::Error`. Все последующие операции с `x` молча пропускают вторичные ошибки типизации, чтобы программист видел первопричину, а не десятки производных ошибок.
4. **Многоточечные аннотации (`MultiSpan`):** для ошибок владения, заимствования и типов компилятор всегда показывает минимум две точки в коде: где было объявлено ограничение/первое действие (`---` вторичная метка) и где произошло нарушение (`^^^` первичная метка).
5. **Детерминизм и воспроизводимость:** диагностики сортируются по `(FileId, start_byte, code)`, что гарантирует побитово идентичный вывод на Windows, Linux и macOS.

---

## 2. Архитектура диагностического движка (`aps_diagnostics`)

### 2.1. Представление исходного кода и координат (`SourceMap` и `Span`)

Каждый загруженный в компилятор исходный файл `.aps` регистрируется в глобальном реестре `SourceMap` и получает компактный дескриптор `FileId(u32)`.

```
+-----------------------------------------------------------------------+
|                              SourceMap                                |
|  files: Vec<SourceFile>                                               |
|    +-- SourceFile {                                                   |
|          id: FileId(0),                                               |
|          path: "src/billing.aps",                                     |
|          source: String,                                              |
|          line_starts: Vec<u32> // Смещения байтов начала каждой строки|
|        }                                                              |
+-----------------------------------------------------------------------+
```

- **`Span`:** компактная 12-байтовая структура `{ file_id: FileId, start: u32, end: u32 }`, хранящая полуоткрытый байтовый интервал `[start, end)` в UTF-8 буфере файла.
- **Вычисление строки и столбца (`LineCol`):** при формировании отчёта метод `SourceFile::lookup_line_col(byte_offset)` выполняет бинарный поиск по массиву `line_starts` за $O(\log L)$, где $L$ — число строк файла, и вычисляет 1-индексированный номер строки и 1-индексированный номер колонки в кодовых точках Unicode.
- **`DiagnosticLabel`:** аннотированный интервал кода, содержащий:
  - `style: LabelStyle` (`Primary` — отрисовывается символами `^`, или `Secondary` — отрисовывается символами `-`);
  - `span: Span`;
  - `message: String` — пояснительная подпись непосредственно под подчёркнутым фрагментом.

### 2.2. Уровни серьёзности (`DiagnosticLevel`)

| Уровень | Префикс в выводе | Цвет терминала (ANSI) | Влияние на сборку (`aps build` / `aps check`) |
| :--- | :--- | :--- | :--- |
| **`Error`** | `error[E0xxx]:` | Ярко-красный жирный (`\x1b[1;31m`) | Блокирует генерацию бинарного файла, код возврата `1` |
| **`Warning`** | `warning[W0xxx]:` | Ярко-жёлтый жирный (`\x1b[1;33m`) | Не блокирует сборку (если не передан флаг `--deny-warnings`) |
| **`Note`** | `= note:` | Голубой/белый (`\x1b[1;36m`) | Дополнительный технический контекст к ошибке или предупреждению |
| **`Help`** | `= help:` | Ярко-зелёный (`\x1b[1;32m`) | Конкретная рекомендация или патч для исправления кода |

### 2.3. Структура объекта `Diagnostic`

```rust
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: &'static str,           // Например, "E0042"
    pub title: String,                // Например, "cannot modify a read-only value"
    pub primary_span: Span,
    pub labels: Vec<DiagnosticLabel>, // Первичные (^^^) и вторичные (---) метки
    pub notes: Vec<String>,           // Строки `= note: ...`
    pub helps: Vec<String>,           // Строки `= help: ...`
    pub suggestion: Option<CodeFix>,  // Автоматическая замена для IDE / `aps fix`
}
```

---

## 3. Эталонный формат сообщения об ошибке и схема JSON-вывода

### 3.1. Человекочитаемый формат (Standard Terminal Format)

Ниже представлен нормативный эталонный формат вывода компилятора APS (на примере ошибки модификации параметра с правом только для чтения `E0042`):

```text
error[E0042]: cannot modify a read-only value
 --> src/billing.aps:8:5
  |
7 | fn apply_bonus(read driver: DriverAccount) {
  |                ----------- `driver` is passed with `read` capability here
8 |     driver.balance += 100
  |     ^^^^^^^^^^^^^^^^^^^^^ cannot mutate field `balance` through a `read` capability
  |
  = note: `read` parameters grant shared immutable access only
  = help: change the parameter signature to `write driver: DriverAccount` if mutation is intended
```

**Правила форматирования:**
1. Первая строка всегда имеет вид `<level>[<code>]: <title>`.
2. Вторая строка указывает путь к файлу, 1-индексированную строку и 1-индексированный столбец начала первичного спана: ` --> <path>:<line>:<col>`.
3. Левое поле с номерами строк выравнивается по ширине максимального отображаемого номера строки и отделяется вертикальной чертой ` | `.
4. Вторичные спаны (контекст объявления) подчёркиваются дефисами `---`, а первичный спан ошибки — циркумфлексами `^^^`.
5. Завершают блок строки `= note:` и `= help:`.

### 3.2. Машиночитаемый формат (`aps check --error-format=json`)

При передаче флага `--error-format=json` компилятор выводит поток объектов JSON Lines (по одному объекту на строку), совместимый с языковым сервером `aps-lsp`:

```json
{
  "level": "error",
  "code": "E0042",
  "message": "cannot modify a read-only value",
  "file": "src/billing.aps",
  "line": 8,
  "column": 5,
  "end_line": 8,
  "end_column": 26,
  "labels": [
    {
      "style": "secondary",
      "file": "src/billing.aps",
      "line": 7,
      "column": 16,
      "end_line": 7,
      "end_column": 27,
      "message": "`driver` is passed with `read` capability here"
    },
    {
      "style": "primary",
      "file": "src/billing.aps",
      "line": 8,
      "column": 5,
      "end_line": 8,
      "end_column": 26,
      "message": "cannot mutate field `balance` through a `read` capability"
    }
  ],
  "notes": [
    "`read` parameters grant shared immutable access only"
  ],
  "helps": [
    "change the parameter signature to `write driver: DriverAccount` if mutation is intended"
  ]
}
```

---

## 4. Категория 1: Синтаксические и лексические ошибки (`E0001`–`E0039`)

Ошибки данной категории обнаруживаются на этапах **Lexer** и **Parser** при нарушении лексической структуры или грамматики EBNF языка APS.

### 4.1. Сводная таблица кодов `E0001`–`E0039`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0001` | `InvalidCharacter` | `unexpected character in source file` | Неизвестный символ вне строкового литерала или комментария | Lexer |
| `E0002` | `UnterminatedString` | `unterminated string literal` | Строковый литерал `"` не закрыт до конца строки или файла | Lexer |
| `E0003` | `InvalidEscapeSequence` | `invalid escape sequence in literal` | Неизвестная escape-последовательность (не `\n`, `\r`, `\t`, `\\`, `\"`, `\0`, `\xNN`, `\u{NNNN}`) | Lexer |
| `E0004` | `MalformedNumberLiteral` | `invalid numeric literal` | Некорректная запись числа (`0x`, `12.34.56`, пустая экспонента `1e`) | Lexer |
| `E0005` | `InvalidCharLiteral` | `invalid character literal` | Пустой символьный литерал `''` или более одной кодовой точки в `'ab'` | Lexer |
| `E0006` | `UnterminatedBlockComment`| `unterminated block comment` | Многострочный комментарий `/*` не имеет закрывающей пары `*/` | Lexer |
| `E0010` | `UnexpectedToken` | `unexpected token in expression or statement` | Встречен токен, не допустимый грамматикой в текущей позиции | Parser |
| `E0011` | `UnclosedDelimiter` | `unclosed delimiter` | Открывающая скобка `(`, `[` или `{` не закрыта парной скобкой | Parser |
| `E0012` | `ExpectedIdentifier` | `expected identifier` | После `fn`, `let`, `var`, `struct`, `enum`, `mod` отсутствует имя | Parser |
| `E0013` | `ExpectedTypeAnnotation` | `expected type annotation` | Пропущен тип после `:` в параметре функции, поле структуры или `const` | Parser |
| `E0014` | `ExpectedBlock` | `expected block '{ ... }'` | Тело функции, `if`, `while`, `for`, `region` не заключено в `{ ... }` | Parser |
| `E0015` | `InvalidAssignmentTarget` | `invalid left-hand side of assignment` | Левая часть присваивания не является переменной, полем или индексом (`a + b = 5`) | Parser |
| `E0016` | `MissingMatchArms` | `match expression must have at least one arm` | Пустое тело конструкции `match` или синтаксическая ошибка в ветви `=>` | Parser |
| `E0017` | `DuplicateStructFieldInit`| `field initialized more than once in struct literal` | В литерале структуры одно и то же поле указано дважды | Parser |
| `E0018` | `ReservedKeywordIdentifier`| `cannot use reserved keyword as identifier` | Использование ключевого слова (`region`, `take`, `write`, `shared`) как имени | Parser |
| `E0019` | `TopLevelStatementNotAllowed`| `executable statement is not allowed at module top level` | Инструкция `let`/`var`/`if` вне тела функции на верхнем уровне модуля | Parser |
| `E0020` | `MalformedAttribute` | `invalid attribute syntax` | Некорректный синтаксис атрибута `@derive(...)` или `@repr(...)` | Parser |

### 4.2. Развёрнутые примеры диагностического вывода (Категория 1)

#### Пример `E0002`: Незакрытый строковый литерал
```text
error[E0002]: unterminated string literal
 --> src/main.aps:4:16
  |
4 |     let host = "192.168.1.100
  |                ^^^^^^^^^^^^^^ string literal starts here and is not closed before end of line
  |
  = help: add a closing double quote `"` at the end of the string literal
```

#### Пример `E0011`: Незакрытый разделитель блока
```text
error[E0011]: unclosed delimiter `{`
 --> src/parser_test.aps:12:1
  |
3 | fn compute_checksum(data: read Slice<u8>) -> u32 {
  |                                                  - unclosed `{` opened here
...
12| 
  | ^ expected `}` to close function body before end of file
  |
  = help: insert `}` at line 12 to close the block opened at line 3
```

---

## 5. Категория 2: Ошибки прав доступа — Capability Errors (`E0040`–`E0099`)

Ошибки данной категории обнаруживаются проходом **AMA Capability Pass** при нарушении правил доступа второго класса (`read`, `write`, `take`, `shared`) и мутабельности привязок (`let` против `var`).

### 5.1. Сводная таблица кодов `E0040`–`E0099`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0040` | `ReadWriteConflict` | `cannot borrow value as 'write' because it is already borrowed as 'read'` | Попытка получить `write`-доступ к пути, для которого активно `read`-заимствование | AMA Capability Pass |
| `E0041` | `DoubleWriteConflict` | `cannot borrow value as 'write' more than once at a time` | Попытка создать два пересекающихся эксклюзивных `write`-заимствования | AMA Capability Pass |
| `E0042` | `MutateReadOnlyCapability` | `cannot modify a read-only value` | Запись в переменную/поле или вызов `write`-метода через параметр `read T` | AMA Capability Pass |
| `E0043` | `MutateSharedValue` | `cannot mutate data inside 'Shared<T>'` | Попытка модифицировать данные внутри замороженного объекта Уровня 4 (`Shared<T>`) | AMA Capability Pass |
| `E0044` | `TakeFromBorrowedValue` | `cannot transfer ownership ('take') from a borrowed capability` | Попытка применить `take` к параметру, полученному по `read T` или `write T` | AMA Capability Pass |
| `E0045` | `WriteBorrowOfImmutable` | `cannot borrow immutable binding as 'write'` | Попытка передать `write x` для переменной, объявленной через `let` вместо `var` | AMA Capability Pass |
| `E0046` | `AssignToImmutableBinding`| `cannot assign twice to immutable variable` | Повторное присваивание или `+=` переменной, объявленной через `let` | AMA Capability Pass |
| `E0047` | `CapabilityMismatchAtCall`| `capability annotation at call site does not match parameter signature` | Функция ожидает `write T` или `take T`, но в вызове передан `read` или опущен маркер | AMA Capability Pass |
| `E0048` | `WriteMethodOnReadReceiver`| `cannot call mutating method on a read-only receiver` | Вызов метода с сигнатурой `fn mutate(write self)` на `let`-объекте или `read`-ссылке | AMA Capability Pass |
| `E0049` | `OverlappingSliceBorrow` | `cannot borrow indexed container as 'write' multiple times without disjoint proof` | Одновременное заимствование `write arr[i]` и `write arr[j]` без `split_at_write` | AMA Capability Pass |

### 5.2. Развёрнутые примеры диагностического вывода (Категория 2)

#### Пример `E0042`: Попытка изменить значение с правом только для чтения (`read`)
```text
error[E0042]: cannot modify a read-only value
 --> src/billing.aps:8:5
  |
7 | fn apply_bonus(read driver: DriverAccount) {
  |                ----------- `driver` is passed with `read` capability here
8 |     driver.balance += 100
  |     ^^^^^^^^^^^^^^^^^^^^^ cannot mutate field `balance` through a `read` capability
  |
  = note: `read` parameters grant shared immutable access only
  = help: change the parameter signature to `write driver: DriverAccount` if mutation is intended
```

#### Пример `E0047`: Несовпадение маркера права доступа в точке вызова
```text
error[E0047]: capability annotation at call site does not match parameter signature
 --> src/engine.aps:14:17
  |
4 | fn reset_turbines(write ctrl: TurbineController) {
  |                   ----------------------------- parameter declared as `write TurbineController` here
...
14|     reset_turbines(read controller)
  |                    ^^^^^^^^^^^^^^^ expected `write controller`, found `read controller`
  |
  = note: APS requires explicit capability agreement between caller and callee
  = help: replace `read controller` with `write controller` at the call site
```

---

## 6. Категория 3: Ошибки типизации — Type Errors (`E0100`–`E0199`)

Ошибки данной категории обнаруживаются на этапах **Semantic Analysis** и **Type Checker** при нарушении строгой статической системы типов APS.

### 6.1. Сводная таблица кодов `E0100`–`E0199`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0100` | `TypeMismatch` | `mismatched types` | Тип выражения не совпадает с ожидаемым типом переменной, поля или аргумента | Type Checker |
| `E0101` | `ReturnTypeMismatch` | `return type does not match function signature` | Тип выражения в `return` (или тела функции) не совпадает с объявленным `-> T` | Type Checker |
| `E0102` | `MissingReturnValue` | `non-void function must return a value on all control-flow paths` | Функция с возвращаемым типом `T != ()` завершается без `return` на одной из ветвей | Type Checker |
| `E0103` | `ImplicitNumericConversion`| `implicit numeric conversion is forbidden in APS` | Попытка присвоить `i32` в `i64` или `i64` в `f64` без явного оператора `as` | Type Checker |
| `E0104` | `NonBooleanCondition` | `condition expression in 'if' or 'while' must have type 'bool'` | Использование числа или строки в условии `if count { ... }` вместо `bool` | Type Checker |
| `E0105` | `NonExhaustiveMatch` | `non-exhaustive patterns in 'match' expression` | В конструкции `match` обработаны не все варианты перечисления `enum` | Type Checker |
| `E0106` | `ArityMismatch` | `function called with wrong number of arguments` | Количество переданных аргументов не совпадает с числом параметров функции | Type Checker |
| `E0107` | `UnknownStructField` | `no field with this name on struct type` | Обращение к несуществующему полю структуры или пропуск обязательного поля | Type Checker |
| `E0108` | `TraitBoundNotSatisfied` | `type does not satisfy required trait bound` | Обобщённый параметр типа `T` не реализует требуемый интерфейс (`Hash`, `Eq`, `Ord`) | Type Checker |
| `E0109` | `InvalidTypeCast` | `invalid type cast with 'as'` | Попытка привести несовместимые типы через `as` (например, `String as i64`) | Type Checker |
| `E0110` | `IncompatibleBranchTypes` | `'if' and 'else' branches have incompatible types` | Ветви выражения `if/else` или `match` возвращают значения разных типов | Type Checker |
| `E0111` | `TryOperatorTypeMismatch` | `the '?' operator can only be applied to 'Result<T, E>' or 'Option<T>'` | Применение оператора `?` к типу, не являющемуся `Result` или `Option`, либо несовместимый `E` | Type Checker |
| `E0112` | `RecursiveStructWithoutIndirection`| `recursive struct type has infinite size` | Структура напрямую содержит поле собственного типа по значению без `Box<T>` или `RegionHandle<T>` | Type Checker |
| `E0113` | `CannotInferType` | `type annotations needed: cannot infer type unambiguously` | Пустая коллекция `Vec::new()` или обобщённый вызов без достаточного контекста для вывода типа | Type Checker |

### 6.2. Развёрнутые примеры диагностического вывода (Категория 3)

#### Пример `E0103`: Запрет неявного приведения числовых типов
```text
error[E0103]: implicit numeric conversion is forbidden in APS
 --> src/physics.aps:6:22
  |
5 |     let packet_size: i32 = 1400
  |                      --- `packet_size` has type `i32`
6 |     let total_bytes: i64 = packet_size
  |                      ---   ^^^^^^^^^^^ expected `i64`, found `i32`
  |                      |
  |                      expected due to this type annotation
  |
  = note: APS does not perform implicit widening or narrowing numeric conversions
  = help: use an explicit cast: `packet_size as i64`
```

#### Пример `E0105`: Неполное покрытие вариантов `enum` в конструкции `match`
```text
error[E0105]: non-exhaustive patterns in `match` expression: `ConnState::TimedOut` not covered
 --> src/client.aps:15:11
  |
2 | enum ConnState { Connected, Disconnected, TimedOut }
  | ---------------------------------------------------- `ConnState` defined here
...
15|     match state {
  |           ^^^^^ pattern `ConnState::TimedOut` is not handled
16|         ConnState::Connected => handle_active(),
17|         ConnState::Disconnected => reconnect()
18|     }
  |
  = help: add a match arm for `ConnState::TimedOut` or a wildcard arm `_ => ...`
```

---

## 7. Категория 4: Ошибки владения — Ownership Errors (`E0200`–`E0299`)

Ошибки данной категории обнаруживаются проходом **AMA Liveness & Move Pass** при нарушении инвариантов единственного владельца, инициализации переменных и перемещения значений.

### 7.1. Сводная таблица кодов `E0200`–`E0299`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0200` | `UseOfUninitializedVariable` | `use of possibly uninitialized variable` | Чтение переменной до того, как ей было присвоено значение на всех ветвях CFG | AMA Liveness & Move Pass |
| `E0201` | `DuplicateOwnershipTransfer` | `cannot transfer ownership of value more than once` | Повторное перемещение (`take`) переменной, уже перешедшей в состояние `Moved` | AMA Liveness & Move Pass |
| `E0202` | `UseOfMaybeMovedValue` | `use of possibly moved value` | Переменная перемещена в одной из ветвей `if`/`match` или в теле цикла и читается после | AMA Liveness & Move Pass |
| `E0203` | `MoveWhileBorrowed` | `cannot move out of value because it is currently borrowed` | Попытка выполнить `take x` в момент, когда существует живая ссылка `read x` или `write x` | AMA Liveness & Move Pass |
| `E0204` | `UseAfterPartialMove` | `cannot use or move whole struct after partial move of its field` | Одно из полей структуры было перемещено (`take s.field`), после чего используется вся `s` | AMA Liveness & Move Pass |
| `E0205` | `MoveConflictsWithDefer` | `cannot move variable because it is captured by a 'defer' block` | Переменная захвачена в блоке `defer`, но перемещается через `take` до конца скоупа | AMA Liveness & Move Pass |
| `E0206` | `MoveOutOfIndexedCollection` | `cannot move element out of indexed collection without replacement` | Попытка переместить не-`Copy` элемент по индексу `let x = take vec[i]` | AMA Liveness & Move Pass |
| `E0207` | `LoopMoveInIteration` | `value moved in previous iteration of loop` | Внутри тела цикла `while`/`for` внешняя переменная перемещается через `take` без реинициализации | AMA Liveness & Move Pass |
| `E0208` | `CyclicStrongOwnership` | `cannot construct cyclic ownership in Single-Owner Heap Tier` | Попытка замкнуть цикл владения без использования `region`/`RegionHandle` или `Weak<T>` | AMA Liveness & Move Pass |

### 7.2. Развёрнутые примеры диагностического вывода (Категория 4)

#### Пример `E0200`: Использование неинициализированной переменной
```text
error[E0200]: use of possibly uninitialized variable `status_code`
 --> src/handler.aps:11:19
  |
4 |     var status_code: u16
  |         ----------- declared without initial value here
5 |     if is_authorized {
6 |         status_code = 200
7 |     }
  |     - if `is_authorized` is `false`, `status_code` remains uninitialized
...
11|     send_response(status_code)
  |                   ^^^^^^^^^^^ use of possibly uninitialized `status_code`
  |
  = help: initialize `status_code` at declaration (`var status_code: u16 = 403`) or assign it in an `else` branch
```

#### Пример `E0207`: Перемещение внешней переменной внутри тела цикла
```text
error[E0207]: value `template` moved in previous iteration of loop
 --> src/worker.aps:7:22
  |
4 |     let template = String::from("job_header")
  |         -------- move occurs because `template` has type `String`, which does not implement `Copy`
5 |     for id in 0..10 {
6 |         // На второй итерации (id == 1) `template` уже будет в состоянии `Moved`!
7 |         dispatch_job(take template, id)
  |                      ^^^^^^^^^^^^^ ownership moved here inside loop body
8 |     }
  |
  = help: pass `read template` if `dispatch_job` only reads the string, or pass `take template.clone()`
```

---

## 8. Категория 5: Безопасность памяти, регионы и времена жизни (`E0300`–`E0399`)

Ошибки данной категории обнаруживаются проходами **AMA Lifetime Analysis** и **AMA Region Escape Pass** при попытке создать висячие ссылки, использовать значение после перемещения или вынести данные арены за пределы блока `region`.

### 8.1. Сводная таблица кодов `E0300`–`E0399`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0301` | `UseAfterMove` | `use of moved value` | Чтение или вызов метода переменной после передачи её владения через `take` | AMA Liveness & Move Pass |
| `E0302` | `ReturnLocalReference` | `cannot return reference to local variable` | Возврат `read`/`write` представления, ссылающегося на локальную переменную стекового кадра | AMA Lifetime Analysis |
| `E0303` | `BorrowedValueDoesNotLiveLongEnough` | `borrowed value does not live long enough` | Ссылка переживает объект-владелец, созданный во вложенном блоке `{ ... }` | AMA Lifetime Analysis |
| `E0304` | `GuardReferenceEscape` | `cannot return or store reference to data guarded by local lock guard` | Ссылка на данные внутри `MutexGuard` или `RwLockGuard` переживает сам охранный объект | AMA Lifetime Analysis |
| `E0305` | `RegionEscape` | `region-allocated value or handle cannot escape 'region' block` | Возврат `RegionHandle<T>` или ссылки из блока `region` или запись во внешнюю переменную | AMA Region Escape Pass |
| `E0306` | `RegionHandleMismatch` | `region handle mismatch: handle belongs to a different region` | Попытка разыменовать `RegionHandle<T>` от `arena_a` через `arena_b` | AMA Region Escape Pass |
| `E0307` | `AmbiguousReturnOrigin` | `returned view origin outlives one of its candidate source parameters` | Возвращённый из функции срез `read` используется после уничтожения одного из входных источников | AMA Lifetime Analysis |
| `E0308` | `TemporaryValueDroppedWhileBorrowed`| `temporary value dropped while still borrowed` | Создание ссылки на временное выражение, уничтожаемое в конце инструкции | AMA Lifetime Analysis |
| `E0309` | `RegionDestroyedWhileBorrowed`| `cannot access region handle after region reset or exit` | Обращение к данным региона после завершения его времени жизни | AMA Region Escape Pass |

### 8.2. Развёрнутые примеры диагностического вывода (Категория 5)

#### Пример `E0301`: Обращение к объекту после передачи владения (`take`)
```text
error[E0301]: use of moved value `packet`
 --> src/network.aps:9:19
  |
7 |     let packet = vec![0xDE, 0xAD, 0xBE, 0xEF]
  |         ------ move occurs because `packet` has type `Vec<u8>`, which does not implement `Copy`
8 |     send_packet(take packet)
  |                 ----------- ownership of `packet` was transferred (`take`) here
9 |     println_usize(packet.len())
  |                   ^^^^^^ value used here after ownership transfer
  |
  = help: read the length before transferring ownership, or change `send_packet` to accept `read Vec<u8>`
```

#### Пример `E0305`: Побег дескриптора из лексического региона (`region`)
```text
error[E0305]: region-allocated handle `root` cannot escape `region ast_mem`
 --> src/parser_arena.aps:9:16
  |
7 |     region ast_mem {
  |            ------- `ast_mem` region lifetime begins here
8 |         let root = ast_mem.alloc(AstNode { token_id: 7 })
9 |         return root
  |                ^^^^ `root` escapes the `region` block here
10|     }
  |     - `ast_mem` is deallocated in bulk at the end of this block
  |
  = help: extract a self-contained value (such as `u32` or an owned `struct`) before exiting `region ast_mem`
```

---

## 9. Категория 6: Многопоточность и асинхронность (`E0400`–`E0499`)

Ошибки данной категории обнаруживаются проходом **AMA Concurrency Pass** при проверке потокобезопасности (`Send` / `Sync`), границ асинхронных задач (`async`/`await`, `task_scope`) и отсутствия гонок данных.

### 9.1. Сводная таблица кодов `E0400`–`E0499`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0400` | `AwaitOutsideAsync` | `'await' expression is only allowed inside an 'async' function or block` | Использование `await` в обычной синхронной функции `fn` | Semantic Analysis |
| `E0401` | `DetachedTaskBorrowEscape` | `borrowed value does not live long enough for detached async task` | Захват локальной ссылки `read` или `write` в `async::spawn_detached` или `thread::spawn` | AMA Concurrency Pass |
| `E0402` | `ConcurrentMutableTaskCapture`| `cannot capture variable as 'write' in multiple concurrent tasks` | Две задачи в `task_scope` или `thread::scope` одновременно захватывают один путь по `write` | AMA Concurrency Pass |
| `E0403` | `UnsynchronizedSharedMutation`| `cannot share mutable state between threads without synchronization` | Попытка разделить изменяемую переменную между потоками без `Mutex<T>` / `Atomic*` | AMA Concurrency Pass |
| `E0404` | `TypeNotSend` | `type cannot be sent between threads safely ('Send' is not satisfied)` | Попытка передать `RegionHandle<T>` или `*mut T` в другой поток или `spawn` | AMA Concurrency Pass |
| `E0405` | `TypeNotSync` | `type cannot be shared across threads safely ('Sync' is not satisfied)` | Попытка разделить по `read` между потоками тип с несинхронизированным состоянием | AMA Concurrency Pass |
| `E0406` | `LockHeldAcrossAwait` | `non-async 'MutexGuard' cannot be held across an 'await' suspension point` | Удержание блокировки потокового `std::sync::MutexGuard` через точку приостановки `await` | AMA Concurrency Pass |
| `E0407` | `TaskScopeHandleEscape` | `'scope' handle cannot escape 'task_scope' closure` | Попытка сохранить параметр `scope` за пределами блока `task_scope` | AMA Concurrency Pass |

### 9.2. Развёрнутые примеры диагностического вывода (Категория 6)

#### Пример `E0404`: Попытка передать потоко-зависимый дескриптор региона в другой поток
```text
error[E0404]: `RegionHandle<String>` cannot be sent between threads safely (`Send` is not satisfied)
 --> src/region_thread.aps:5:22
  |
4 |         let h = local_arena.alloc(String::from("thread_unsafe"))
  |             - `h` has type `RegionHandle<String>`, which is tied to thread-local `region local_arena`
5 |         let worker = thread::spawn(move || {
  |                      ^^^^^^^^^^^^^ `RegionHandle<String>` cannot cross thread boundaries
  |
  = note: `region` arenas use thread-local lock-free bump allocation and cannot be accessed from other threads
  = help: convert the data into an owned type or freeze it with `share(...)` (`Shared<T>`) before spawning threads
```

#### Пример `E0406`: Удержание синхронного `MutexGuard` через точку приостановки `await`
```text
error[E0406]: non-async `MutexGuard<SessionMap>` cannot be held across an `await` suspension point
 --> src/server.aps:19:18
  |
17|     let guard = state_mutex.lock()
  |         ----- synchronous `std::sync::MutexGuard` acquired here
18|     guard.insert(client_id)
19|     let packet = await socket.read_frame()
  |                  ^^^^^^^^^^^^^^^^^^^^^^^^^ task suspends at `await` while still holding OS thread lock `guard`
20|     println_usize(guard.len())
  |                   ----- `guard` is used after `await` here
  |
  = note: holding an OS mutex across `await` can deadlock worker threads in the async executor
  = help: drop `guard` before `await` (using an inner `{ ... }` block) or use `std::async::AsyncMutex<T>`
```

---

## 10. Категория 7: Модули, импорты и области видимости (`E0500`–`E0599`)

Ошибки данной категории обнаруживаются на этапе **Semantic Analysis & Name Resolution** при разрешении путей модулей, проверке видимости (`pub` против приватных символов) и обнаружении коллизий имён.

### 10.1. Сводная таблица кодов `E0500`–`E0599`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0500` | `UnresolvedIdentifier` | `cannot find value or function in this scope` | Обращение к необъявленной переменной, константе или функции | Semantic Analysis |
| `E0501` | `UnresolvedType` | `cannot find type or trait in this scope` | Использование неизвестного имени типа в аннотации | Semantic Analysis |
| `E0502` | `UnresolvedModule` | `cannot find module in package or standard library` | Директива `import` или `mod` ссылается на несуществующий модуль или файл | Semantic Analysis |
| `E0503` | `PrivateSymbolAccess` | `symbol is private and cannot be accessed from outside its defining module` | Обращение к функции, типу или полю структуры без модификатора `pub` из другого модуля | Semantic Analysis |
| `E0504` | `PrivateStructFieldConstruction`| `cannot construct struct with private fields outside its defining module` | Попытка создать экземпляр структуры `Foo { ... }` напрямую при наличии приватных полей | Semantic Analysis |
| `E0505` | `DuplicateDefinition` | `the name is defined multiple times in the same namespace` | Два типа, две функции или два модуля с одинаковым именем в одной области видимости | Semantic Analysis |
| `E0506` | `CircularModuleDependency` | `circular module initialization dependency detected` | Циклическая зависимость констант инициализации между модулями `A -> B -> A` | Semantic Analysis |
| `E0507` | `BreakContinueOutsideLoop` | `'break' or 'continue' cannot be used outside of a loop` | Использование `break` или `continue` вне `while` / `for` / `loop` | Semantic Analysis |
| `E0508` | `AmbiguousGlobImport` | `name imported via multiple glob imports is ambiguous` | Символ экспортируется сразу двумя импортированными модулями | Semantic Analysis |

### 10.2. Развёрнутые примеры диагностического вывода (Категория 7)

#### Пример `E0500`: Необъявленный идентификатор (с подсказкой по расстоянию Левенштейна)
```text
error[E0500]: cannot find value `connection_timeout` in this scope
 --> src/config.aps:14:21
  |
11|     let conn_timeout: u64 = 5000
  |         ------------ a variable with a similar name exists here
...
14|     set_socket_wait(connection_timeout)
  |                     ^^^^^^^^^^^^^^^^^^ not found in this scope
  |
  = help: did you mean `conn_timeout`?
```

#### Пример `E0503`: Обращение к приватному полю или функции другого модуля
```text
error[E0503]: function `derive_master_key` is private and cannot be called from module `api`
 --> src/api/auth.aps:9:15
  |
9 |     let key = crypto::vault::derive_master_key(read seed)
  |               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ function `derive_master_key` is not marked `pub`
  |
 ::: src/crypto/vault.aps:22:1
  |
22| fn derive_master_key(read seed: Slice<u8>) -> Key256 {
  | ---------------------------------------------------- defined as private here
  |
  = help: mark the function as public (`pub fn derive_master_key(...)`) in `src/crypto/vault.aps` if external access is intended
```

---

## 11. Категория 8: Безопасность FFI и блоков `unsafe` (`E0600`–`E0699`)

Ошибки данной категории обнаруживаются проходом **AMA FFI & Unsafe Safety Pass** при взаимодействии с внешним кодом `extern "C"`, сырыми указателями `*const T` / `*mut T` и низкоуровневыми атрибутами представления памяти.

### 11.1. Сводная таблица кодов `E0600`–`E0699`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0600` | `RawPointerDerefOutsideUnsafe`| `dereferencing a raw pointer requires an 'unsafe' block` | Разыменование `*ptr` (`*const T` или `*mut T`) в обычном безопасном коде | AMA FFI Safety Pass |
| `E0601` | `ExternCallOutsideUnsafe` | `call to 'extern "C"' or 'unsafe' function requires an 'unsafe' block` | Вызов внешней C-функции или `unsafe fn` вне блока `unsafe { ... }` | AMA FFI Safety Pass |
| `E0602` | `NonFFISafeTypeInExtern` | `type is not FFI-safe for 'extern "C"' signature` | Передача по значению в `extern "C"` типа APS без `@repr(C)` (например, `String` или `Vec<T>`) | AMA FFI Safety Pass |
| `E0603` | `UnionFieldReadOutsideUnsafe` | `access to field of 'union' requires an 'unsafe' block` | Чтение поля объединения `union` вне блока `unsafe` | AMA FFI Safety Pass |
| `E0604` | `InvalidCapabilitiesInExtern`| `'extern "C"' declarations must use C ABI raw pointers or '@repr(C)' values` | Использование высокоуровневых дескрипторов `RegionHandle<T>` в сигнатуре `extern "C"` | AMA FFI Safety Pass |
| `E0605` | `ConflictingReprAttribute` | `conflicting memory layout '@repr' attributes on type` | Одновременное указание несовместимых атрибутов `@repr(C)` и `@repr(packed)` с неверным выравниванием | AMA FFI Safety Pass |

### 11.2. Развёрнутые примеры диагностического вывода (Категория 8)

#### Пример `E0600`: Разыменование сырого указателя вне блока `unsafe`
```text
error[E0600]: dereferencing a raw pointer requires an `unsafe` block
 --> src/driver_mmio.aps:6:15
  |
5 | fn read_status_register(reg_addr: *const u32) -> u32 {
6 |     let val = *reg_addr
  |               ^^^^^^^^^ dereference of raw pointer `*const u32` outside `unsafe` block
  |
  = note: raw pointers may be null, unaligned, or point to unmapped memory
  = help: wrap the dereference in `unsafe { *reg_addr }` after verifying hardware address validity
```

#### Пример `E0602`: Передача несовместимого с C ABI типа в `extern "C"`
```text
error[E0602]: type `String` is not FFI-safe for `extern "C"` signature
 --> src/c_bridge.aps:4:25
  |
3 | extern "C" {
4 |     fn c_log_message(s: String)
  |                         ^^^^^^ `String` has an APS-internal memory layout not guaranteed across C ABI
5 | }
  |
  = help: pass a raw pointer and length `(ptr: *const u8, len: usize)` or a null-terminated `*const u8` instead
```

---

## 12. Категория 9: Ошибки компоновки и кодогенерации — Linker Errors (`E0700`–`E0799`)

Ошибки данной категории обнаруживаются на финальных этапах **LLVM Backend** и **System Linker (`clang` / `lld-link`)** при сборке исполняемого файла.

### 12.1. Сводная таблица кодов `E0700`–`E0799`

| Код | Символическое имя | Сообщение компилятора | Условие возникновения | Этап |
| :--- | :--- | :--- | :--- | :--- |
| `E0700` | `MissingMainEntryPoint` | `'main' function not found in binary crate` | При сборке исполняемого файла отсутствует точка входа `fn main()` | Codegen / Linker |
| `E0701` | `InvalidMainSignature` | `'main' function has an invalid signature` | Функция `main` принимает параметры или возвращает тип кроме `()`, `i32` или `Result<(), E>` | Codegen / Linker |
| `E0702` | `UnresolvedExternalSymbol` | `unresolved external symbol during linking` | Объявленная в `extern "C"` функция не найдена ни в одной подключённой библиотеке | Linker |
| `E0703` | `DuplicateExportedSymbol` | `duplicate symbol definition across compilation units` | Две функции помечены одинаковым `@export_name("...")` или конфликтуют с C-библиотекой | Linker |
| `E0704` | `TargetToolchainNotFound` | `LLVM/Clang backend toolchain not found or failed to execute` | Системный компоновщик для выбранного `--target` недоступен или завершился с ошибкой | Codegen / Linker |
| `E0705` | `UnsupportedTargetTriple` | `unsupported compilation target triple` | Передан неизвестный целевой триплет в `--target` | Codegen / Linker |

### 12.2. Развёрнутые примеры диагностического вывода (Категория 9)

#### Пример `E0700`: Отсутствие точки входа `fn main()` в бинарном проекте
```text
error[E0700]: `main` function not found in binary crate `telemetry_node`
 --> src/main.aps:1:1
  |
  = note: every executable APS package must define an entry point `fn main()`
  = help: add `fn main() { ... }` to `src/main.aps`, or compile as a library with `--crate-type=lib`
```

#### Пример `E0702`: Неразрешённый внешний символ при линковке C-библиотеки
```text
error[E0702]: unresolved external symbol `zstd_compress_bound` during linking
 --> src/compression.aps:4:8
  |
4 |     fn zstd_compress_bound(src_size: usize) -> usize
  |        ^^^^^^^^^^^^^^^^^^^ symbol declared in `extern "C"` block here, but not found by linker
  |
  = note: linker `clang` exited with status 1: undefined symbol `zstd_compress_bound`
  = help: link the required native library in `apm.toml` under `[build.native-libs]` or pass `-l zstd`
```

---

## 13. Категория 10: Ошибки времени выполнения и паники — Runtime Errors (`R0001`–`R0099`)

В отличие от ошибок компиляции `E0xxx`, коды `R0001`–`R0099` генерируются минимальным детерминированным рантаймом APS во время выполнения программы, когда срабатывает защитная проверка безопасности, которую невозможно было опровергнуть или доказать чисто статически (например, индексация массива значением, введённым пользователем из сети).

### 13.1. Сводная таблица кодов `R0001`–`R0099`

| Код | Символическое имя | Сообщение рантайма APS | Условие возникновения во время выполнения |
| :--- | :--- | :--- | :--- |
| `R0001` | `IndexOutOfBounds` | `index out of bounds: index is N, but length is L` | Обращение `arr[i]` или `slice[i]` при $i < 0$ или $i \ge \text{len}$ (`KI-003`) |
| `R0002` | `DivisionByZero` | `attempt to divide by zero or compute remainder with a divisor of zero` | Целочисленное деление `a / b` или взятие остатка `a % b` при `b == 0` |
| `R0003` | `IntegerOverflow` | `arithmetic operation overflowed integer bounds` | Переполнение знакового/беззнакового целого в режиме проверяемой арифметики |
| `R0004` | `ExplicitPanic` | `explicit panic invoked` | Вызов макроса/функции `panic("reason")` или `unreachable()` в коде |
| `R0005` | `UnwrapOnNoneOrErr` | `called 'unwrap()' on a 'None' or 'Err' value` | Вызов `.unwrap()` на пустом `Option::None` или ошибочном `Result::Err(e)` |
| `R0006` | `AssertionFailed` | `assertion failed` | Нарушение инварианта `assert(cond)` или `assert_eq(a, b)` в рантайме/тестах |
| `R0007` | `OutOfMemory` | `APS runtime memory allocator exhausted available system memory` | Операционная система отказала в выделении страницы кучи или региона |
| `R0008` | `RegionCapacityExceeded`| `fixed-capacity region exceeded its maximum memory budget` | Превышен жёсткий лимит памяти региона `region name(max_bytes = N)` |
| `R0009` | `DeadlockOrPoisonedLock`| `attempted to acquire a poisoned mutex or recursive lock on same thread` | Поток пытается захватить `Mutex`, чей предыдущий владелец завершился с паникой |
| `R0010` | `StackOverflow` | `thread stack overflow detected` | Бесконечная рекурсия исчерпала стековый сегмент потока (Guard Page Fault) |

### 13.2. Развёрнутые примеры диагностического вывода рантайма (Категория 10)

#### Пример `R0001`: Выход индекса за границы массива во время выполнения
```text
thread 'main' panicked with runtime error[R0001]: index out of bounds: the len is 4, but the index is 9
 --> src/telemetry.aps:18:19
  |
18|     let sample = sensors[index]
  |                  ^^^^^^^^^^^^^^ index 9 is out of bounds for `Vec<f64>` of length 4
  |
  = stack backtrace:
      0: telemetry::read_sensor_at (src/telemetry.aps:18:19)
      1: telemetry::main (src/telemetry.aps:27:5)
  = help: use `sensors.get(index)` returning `Option<read f64>` for fallible bounds checking
```

#### Пример `R0002`: Целочисленное деление на ноль
```text
thread 'worker-2' panicked with runtime error[R0002]: attempt to divide by zero
 --> src/stats.aps:11:20
  |
11|     let avg_rate = total_packets / elapsed_seconds
  |                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ divisor `elapsed_seconds` evaluated to `0`
  |
  = stack backtrace:
      0: stats::compute_throughput (src/stats.aps:11:20)
      1: stats::main (src/stats.aps:21:9)
  = help: guard the division with `if elapsed_seconds != 0` or use `total_packets.checked_div(elapsed_seconds)`
```

---

## 14. Предупреждения компилятора и линтера (`W0001`–`W0020`)

Предупреждения сигнализируют о потенциальных логических недочётах, мёртвом коде или неоптимальном использовании памяти AMA, не блокируя компиляцию (если не активен флаг `--deny-warnings`).

| Код | Символическое имя | Сообщение компилятора | Рекомендация (`= help:`) |
| :--- | :--- | :--- | :--- |
| `W0001` | `UnusedVariable` | `unused variable` | Префиксируйте имя подчёркиванием `_name` или удалите переменную |
| `W0002` | `UnusedMutVar` | `variable does not need to be declared mutable ('var')` | Замените `var x` на неизменяемую привязку `let x` |
| `W0003` | `UnreachableCode` | `unreachable statement after 'return', 'break', or 'panic'` | Удалите недостижимый код после точки безусловного выхода |
| `W0004` | `UnusedImport` | `unused import in module` | Удалите неиспользуемую директиву `import` |
| `W0005` | `UnusedResultMustUse` | `unused 'Result<T, E>' that must be handled` | Обработайте ошибку через `?`, `match` или явно проигнорируйте `let _ = ...` |
| `W0006` | `UnnecessaryClone` | `unnecessary '.clone()' on 'Copy' type or last-use variable` | Передайте значение напрямую или через `take` без лишнего клонирования |
| `W0007` | `WriteCapabilityNotMutated`| `parameter declared as 'write T' is never mutated in function body` | Понизьте требование параметра до `read T` для расширения применимости функции |
| `W0008` | `DeadStructField` | `field of private struct is never read` | Удалите неиспользуемое поле для уменьшения размера структуры в памяти |
| `W0009` | `ConstantCondition` | `condition always evaluates to a constant boolean value` | Упростите условное выражение `if` или `while` |
| `W0010` | `RedundantUnsafeBlock` | `unnecessary 'unsafe' block: body contains no unsafe operations` | Удалите обёртку `unsafe { ... }`, так как все операции внутри безопасны |

### Пример предупреждения `W0007` (Избыточное право доступа `write` без фактической мутации):
```text
warning[W0007]: parameter `account` is declared with `write` capability, but is never mutated
 --> src/audit.aps:5:18
  |
5 | fn print_summary(write account: Account) {
  |                  ^^^^^^^^^^^^^ `account` is only read inside `print_summary`
6 |     println_i64(account.balance)
7 | }
  |
  = note: requesting `write` capability unnecessarily prevents callers from passing `let` bindings or shared references
  = help: change the parameter capability from `write account: Account` to `read account: Account`
```
