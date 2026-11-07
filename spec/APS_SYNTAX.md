# Формальная спецификация синтаксиса и грамматики языка APS (`APS_SYNTAX.md`)

**Версия:** 1.0  
**Назначение:** Однозначное определение лексической и синтаксической структуры языка APS для реализации `lexer`, `parser` и `aps fmt`.

---

## 1. Лексическая структура

Исходный текст программы APS представляет собой последовательность символов UTF-8.

### 1.1. Ключевые слова (Keywords)

В языке APS зарезервированы следующие 36 ключевых слов:

| Категория | Ключевые слова |
|---|---|
| Объявления | `fn`, `let`, `var`, `const`, `struct`, `enum`, `trait`, `impl`, `type`, `module`, `import`, `pub`, `extern` |
| Управление потоком | `if`, `else`, `match`, `while`, `for`, `in`, `loop`, `break`, `continue`, `return`, `defer` |
| Система памяти AMA | `read`, `write`, `take`, `region`, `share`, `shared`, `unsafe` |
| Конкурентность | `async`, `await`, `spawn` |
| Литералы и приведение | `true`, `false`, `as` |

Встроенные имена примитивных типов:
`i8`, `i16`, `i32`, `i64`, `isize`, `u8`, `u16`, `u32`, `u64`, `usize`, `f32`, `f64`, `bool`, `char`, `str`, `String`, `void`, `ptr`.

### 1.2. Операторы и разделители

| Группа | Символы |
|---|---|
| Арифметические | `+`, `-`, `*`, `/`, `%` |
| Присваивание и составные | `=`, `+=`, `-=`, `*=`, `/=`, `%=` |
| Сравнение | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| Логические | `&&`, `\|\|`, `!` |
| Побитовые | `&`, `\|`, `^`, `~`, `<<`, `>>` |
| Диапазоны и доступ | `..`, `..=`, `.`, `::`, `->`, `=>`, `?` |
| Разделители | `(`, `)`, `{`, `}`, `[`, `]`, `,`, `:`, `;` |

### 1.3. Комментарии
- Однострочный комментарий: `//` до символа `\n`.
- Многострочный комментарий: `/* ... */` (с поддержкой вложенности).
- Документирующий комментарий: `///` (для следующего за ним элемента) и `//!` (для текущего модуля).

### 1.4. Литералы
- **Целочисленные литералы:**
  - Десятичные: `0`, `42`, `1_000_000`
  - Шестнадцатеричные: `0xFF`, `0x10_A4`
  - Двоичные: `0b1010_0101`
  - Восьмеричные: `0o755`
  - Опциональные типовые суффиксы: `100i32`, `42u64`, `255u8`
- **Вещественные литералы:**
  - `3.14`, `0.5`, `1.0e-6`, `2.5f32`, `3.14159f64`
- **Строковые литералы:**
  - Заключаются в двойные кавычки `"..."`. Поддерживают управляющие последовательности `\n`, `\r`, `\t`, `\\`, `\"`, `\0`, `\xHH`, `\u{HHHH}`.
- **Символьные литералы:**
  - Заключаются в одинарные кавычки `'a'`, `'Ж'`, `'\n'`.
- **Булевы литералы:**
  - `true`, `false`.

### 1.5. Правила именования (Naming Conventions)
- **Идентификаторы:** начинаются с латинской буквы `a-z`, `A-Z` или `_`, за которыми следуют буквы, цифры `0-9` или `_`.
- **Функции, переменные, модули, поля структур:** `snake_case` (например, `add_balance`, `driver_id`).
- **Структуры, перечисления, типажи, обобщённые параметры:** `PascalCase` (например, `Driver`, `PaymentStatus`, `HashMap`).
- **Константы:** `SCREAMING_SNAKE_CASE` (например, `MAX_BUFFER_SIZE`).

---

## 2. Таблица приоритета операторов

От высшего приоритета (1) к низшему (13):

| Приоритет | Операторы | Ассоциативность | Описание |
|---|---|---|---|
| 1 | `x.f`, `f(x)`, `a[i]`, `x?` | Левая | Доступ к полю, вызов, индексация, проброс ошибки |
| 2 | `-x`, `!x`, `~x`, `read x`, `write x`, `take x`, `await x` | Правая | Унарные операторы и модификаторы доступа |
| 3 | `x as T` | Левая | Явное приведение типов |
| 4 | `*`, `/`, `%` | Левая | Мультипликативные операции |
| 5 | `+`, `-` | Левая | Аддитивные операции |
| 6 | `<<`, `>>` | Левая | Побитовые сдвиги |
| 7 | `&` | Левая | Побитовое И |
| 8 | `^` | Левая | Побитовое исключающее ИЛИ |
| 9 | `\|` | Левая | Побитовое ИЛИ |
| 10 | `..`, `..=` | Неассоциативны | Диапазоны |
| 11 | `==`, `!=`, `<`, `<=`, `>`, `>=` | Неассоциативны | Операторы сравнения |
| 12 | `&&` | Левая | Логическое И (с коротким замыканием) |
| 13 | `\|\|` | Левая | Логическое ИЛИ (с коротким замыканием) |
| 14 | `=`, `+=`, `-=`, `*=`, `/=`, `%=` | Правая | Операторы присваивания |

---

## 3. Формальная грамматика APS в нотации EBNF

```ebnf
Program        = [ ModuleDecl ] { ImportDecl } { Item } ;

ModuleDecl     = "module" ModulePath [ ";" ] ;
ImportDecl     = "import" ModulePath [ ";" ] ;
ModulePath     = Ident { ( "." | "::" ) Ident } ;

Item           = [ "pub" ] ( FnDecl
                           | StructDecl
                           | EnumDecl
                           | ConstDecl
                           | ImplBlock
                           | ExternBlock
                           | TraitDecl ) ;

(* --- Функции --- *)
FnDecl         = [ "async" ] "fn" Ident [ GenericParams ] "(" [ ParamList ] ")" [ "->" Type ] Block ;
ParamList      = Param { "," Param } [ "," ] ;
Param          = Ident ":" [ Capability ] Type
               | "self" [ ":" [ Capability ] Type ] ;
Capability     = "read" | "write" | "take" ;

(* --- Структуры и перечисления --- *)
StructDecl     = "struct" Ident [ GenericParams ] "{" { StructField } "}" ;
StructField    = [ "pub" ] Ident ":" Type [ "," | ";" ] ;

EnumDecl       = "enum" Ident [ GenericParams ] "{" { EnumVariant } "}" ;
EnumVariant    = Ident [ "(" TypeList ")" | "{" { StructField } "}" ] [ "," | ";" ] ;

ImplBlock      = "impl" [ GenericParams ] Ident [ "for" Type ] "{" { [ "pub" ] FnDecl } "}" ;
TraitDecl      = "trait" Ident [ GenericParams ] "{" { FnSig } "}" ;
FnSig          = "fn" Ident [ GenericParams ] "(" [ ParamList ] ")" [ "->" Type ] [ ";" ] ;

ExternBlock    = "extern" [ StringLit ] "{" { FnSig } "}" ;

ConstDecl      = "const" Ident [ ":" Type ] "=" Expr [ ";" ] ;

(* --- Обобщения и типы --- *)
GenericParams  = "<" GenericParam { "," GenericParam } ">" ;
GenericParam   = Ident [ ":" Ident ] ;
TypeList       = Type { "," Type } ;

Type           = [ Capability ] BaseType ;
BaseType       = Ident [ "<" TypeList ">" ]
               | "[" Type [ ";" IntLit ] "]"
               | "fn" "(" [ TypeList ] ")" [ "->" Type ]
               | "shared" BaseType
               | "ptr" "<" Type ">"
               | "void" ;

(* --- Блоки и инструкции --- *)
Block          = "{" { Stmt } [ Expr ] "}" ;

Stmt           = LetStmt
               | VarStmt
               | ConstDecl
               | AssignStmt
               | ReturnStmt
               | BreakStmt
               | ContinueStmt
               | DeferStmt
               | RegionStmt
               | UnsafeStmt
               | WhileStmt
               | ForStmt
               | LoopStmt
               | IfExpr
               | MatchExpr
               | ExprStmt ;

LetStmt        = "let" Ident [ ":" Type ] "=" Expr [ ";" ] ;
VarStmt        = "var" Ident [ ":" Type ] "=" Expr [ ";" ] ;
AssignStmt     = LValue ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) Expr [ ";" ] ;
LValue         = Ident { "." Ident | "[" Expr "]" } ;

ReturnStmt     = "return" [ Expr ] [ ";" ] ;
BreakStmt      = "break" [ ";" ] ;
ContinueStmt   = "continue" [ ";" ] ;
DeferStmt      = "defer" Block ;
RegionStmt     = "region" Ident Block ;
UnsafeStmt     = "unsafe" Block ;

WhileStmt      = "while" Expr Block ;
ForStmt        = "for" Ident "in" Expr Block ;
LoopStmt       = "loop" Block ;

ExprStmt       = Expr [ ";" ] ;

(* --- Выражения --- *)
Expr           = LogicOrExpr ;
LogicOrExpr    = LogicAndExpr { "||" LogicAndExpr } ;
LogicAndExpr   = CompExpr { "&&" CompExpr } ;
CompExpr       = RangeExpr [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) RangeExpr ] ;
RangeExpr      = BitOrExpr [ ( ".." | "..=" ) BitOrExpr ] ;
BitOrExpr      = BitXorExpr { "|" BitXorExpr } ;
BitXorExpr     = BitAndExpr { "^" BitAndExpr } ;
BitAndExpr     = ShiftExpr { "&" ShiftExpr } ;
ShiftExpr      = AddExpr { ( "<<" | ">>" ) AddExpr } ;
AddExpr        = MulExpr { ( "+" | "-" ) MulExpr } ;
MulExpr        = CastExpr { ( "*" | "/" | "%" ) CastExpr } ;
CastExpr       = UnaryExpr { "as" Type } ;

UnaryExpr      = [ "-" | "!" | "~" | "read" | "write" | "take" | "await" | "share" | "spawn" ] PostfixExpr ;

PostfixExpr    = PrimaryExpr { "." Ident [ "(" [ ArgList ] ")" ]
                             | "(" [ ArgList ] ")"
                             | "[" Expr "]"
                             | "?" } ;

ArgList        = Arg { "," Arg } [ "," ] ;
Arg            = [ Capability ] Expr ;

PrimaryExpr    = IntLit
               | FloatLit
               | StringLit
               | CharLit
               | BoolLit
               | Ident [ "::" Ident ] [ StructInit ]
               | "[" [ Expr { "," Expr } [ "," ] ] "]"
               | "(" Expr ")"
               | IfExpr
               | MatchExpr
               | ClosureExpr
               | Block ;

StructInit     = "{" [ FieldInit { ( "," | ";" ) FieldInit } [ "," ] ] "}" ;
FieldInit      = Ident ":" Expr ;

IfExpr         = "if" Expr Block [ "else" ( IfExpr | Block ) ] ;

MatchExpr      = "match" Expr "{" { MatchArm } "}" ;
MatchArm       = Pattern "=>" ( Expr [ "," ] | Block ) ;
Pattern        = "_"
               | Literal
               | Ident [ "." Ident | "::" Ident ] [ "(" [ PatternList ] ")" | "{" [ PatternList ] "}" ] ;
PatternList    = Pattern { "," Pattern } ;

ClosureExpr    = "|" [ ParamList ] "|" [ "->" Type ] ( Block | Expr ) ;
```

---

## 4. Правила форматирования кода (`aps fmt`)

1. **Отступы:** строго 4 пробела (символы табуляции `\t` преобразуются в 4 пробела).
2. **Фигурные скобки:** открывающая скобка `{` пишется на той же строке через один пробел после заголовка объявления или управляющей конструкции (`fn main() {`, `struct Driver {`, `if cond {`).
3. **Точки с запятой:** не ставятся в конце однострочных инструкций и полей структур, кроме случаев записи нескольких инструкций в одной строке.
4. **Пробелы вокруг операторов:** бинарные операторы (`+`, `-`, `=`, `==`, `->`, `=>`) окружаются одним пробелом с обеих сторон; двоеточие типа пишется слитно с именем и через пробел перед типом (`id: i64`).

---

## 5. Синтаксис модулей и стандартная структура проекта APS

Стандартный проект APS, создаваемый командами `aps new my_project` или `apm init`, имеет следующую структуру:

```text
my_project/
├── aps.toml          # Манифест проекта и зависимостей (APS Package Manifest)
├── aps.lock          # Файл фиксации версий и контрольных сумм SHA-256
├── src/
│   ├── main.aps      # Точка входа исполняемого приложения
│   ├── lib.aps       # Корневой модуль библиотеки (опционально)
│   └── domain/
│       └── mod.aps   # Вложенный модуль domain
├── tests/
│   └── basic_test.aps # Модульные и интеграционные тесты
└── target/           # Артефакты компиляции (LLVM IR, объектные и исполняемые файлы)
```

Пример файла `aps.toml`:
```toml
[package]
name = "my_project"
version = "0.1.0"
edition = "2026"
authors = ["APS Developer"]

[dependencies]
```
