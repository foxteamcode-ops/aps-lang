# Справочник: Перечисления и Алгебраические типы в APS (Enums Reference)

Перечисления (`enum`) в **APS** представляют собой мощные размеченные объединения (Tagged Unions / Sum Types). Варианты перечислений могут быть простыми константами, кортежными вариантами с полезной нагрузкой или именованными структурами.

---

## 1. Объявление перечислений

```aps
// Простое перечисление (C-like enum)
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Fatal,
}

// Перечисление с разнородной полезной нагрузкой
pub enum WebEvent {
    PageLoad,
    KeyPress(char),
    MouseMove { x: i32, y: i32 },
    Paste(String),
}
```

---

## 2. Представление в оперативной памяти

В физической памяти экземпляр `enum` размещается непрерывным блоком, состоящим из двух частей:
1. **Дискриминант (Tag):** целое число минимального размера (`u8` для $\le 256$ вариантов), определяющее текущий активный вариант.
2. **Блок полезной нагрузки (Payload):** невыровненный или выровненный буфер размером с максимальный вариант среди всех допустимых.

Оптимизация Null-Pointer Optimization (NPO): для `Option<read T>` или `Option<Box<T>>` компилятор APS использует нулевой указатель `0x0` для представления варианта `None`, делая размер `Option` равным ровно 8 байтам без накладных расходов на тег!

---

## 3. Исчерпывающее сопоставление с образцом (Exhaustive Pattern Matching)

Вся работа со значениями `enum` в безопасном коде осуществляется через оператор `match`. Компилятор проверяет исчерпываемость ветвей (Exhaustiveness Checking):

```aps
fn handle_event(event: WebEvent) {
    match event {
        WebEvent::PageLoad => {
            print("Страница загружена");
        }
        WebEvent::KeyPress(c) => {
            print("Нажата клавиша: " + c.to_string());
        }
        WebEvent::MouseMove { x, y } => {
            print("Мышь перемещена в: " + x.to_string() + ", " + y.to_string());
        }
        WebEvent::Paste(text) => {
            print("Вставлен текст: " + text);
        }
    }
}
```

Если опустить хотя бы один вариант (например `WebEvent::Paste`) и не указать подстановочный знак `_`, компилятор откажется компилировать код:
```
error[E0401]: non-exhaustive patterns: variant 'WebEvent::Paste' not covered
```

---

## 4. Стандартные типы `Option` и `Result`

### 4.1. `Option<T>`
Заменяет концепцию небезопасных нулевых указателей (`null` / `nil` / `nullptr`):
```aps
let found_id: Option<i64> = find_user("alice");
match found_id {
    Some(id) => print("Найден ID: " + id.to_string()),
    None => print("Пользователь не найден"),
}

// Использование методов-хелперов
let safe_id = found_id.unwrap_or(0);
```

### 4.2. `Result<T, E>`
Идиоматическая обработка ошибок без скрытых исключений времени выполнения:
```aps
let file_res = File::open("data.txt");
match file_res {
    Ok(file) => process(file),
    Err(err) => print("Ошибка открытия: " + err.message()),
}
```
