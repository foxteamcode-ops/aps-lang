# Глава 10: Обработка ошибок без Исключений в APS

Язык APS полностью отвергает традиционную модель исключений времени выполнения (C++ / Java `try-catch`), так как скрытые раскрутки стека (Stack Unwinding) разрушают оптимизации компилятора, делают код непредсказуемым по времени и приводят к утечкам ресурсов.

Вместо этого все возможные ошибки в APS являются **явными типами данных**, проверяемыми на этапе компиляции.

---

## 1. Тип `Option<T>` для отсутствующих значений

Вместо опасных указателей `null` или `nil`, приводящих к миллиардным авариям Null Pointer Dereference, используется перечисление `Option<T>`:

```aps
pub enum Option<T> {
    Some(T),
    None,
}
```

### Безопасная работа с `Option`:
```aps
fn find_port(env_var: String) -> Option<u16> {
    if env_var == "PORT" {
        Some(8080)
    } else {
        None
    }
}

// 1. Через match
match find_port("PORT") {
    Some(port) => print("Порт: " + port.to_string()),
    None => print("Используем порт по умолчанию"),
}

// 2. Метод unwrap_or
let port = find_port("HTTP_PORT").unwrap_or(80);
```

---

## 2. Тип `Result<T, E>` для восстановимых ошибок

```aps
pub enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

### Оператор распространения ошибок `?`
Оператор `?` разворачивает значение `Ok(val)` или немедленно возвращает `Err(e)` из текущей функции:

```aps
fn read_config_file(path: String) -> Result<Config, IOError> {
    let mut file = File::open(path)?; // Если ошибка, вернёт Err(IOError)
    let content = file.read_all_to_string()?;
    let config = parse_json_config(content)?;
    Ok(config)
}
```

---

## 3. Пользовательские типы ошибок через `enum`

Идиоматический способ проектирования доменных ошибок — размеченное перечисление:

```aps
pub enum DatabaseError {
    ConnectionLost { host: String, port: u16 },
    QuerySyntaxError(String),
    RecordNotFound(u64),
    PermissionDenied,
}

impl DatabaseError {
    pub fn description(read self) -> String {
        match self {
            DatabaseError::ConnectionLost { host, port } => {
                "Потеряно соединение с " + host + ":" + port.to_string()
            }
            DatabaseError::RecordNotFound(id) => {
                "Запись #" + id.to_string() + " не найдена"
            }
            _ => "Системная ошибка БД",
        }
    }
}
```

---

## 4. Невосстановимые ошибки: `panic`

Для ситуаций нарушения внутренних инвариантов программы (например, повреждение памяти на уровне ядра или исчерпание ОЗУ) используется функция `panic`:

```aps
if system_memory_corrupted() {
    panic("Критический инвариант ядра нарушен!");
}
```
Паника немедленно прерывает выполнение процесса с выводом стек-трейса и кода аварии `R0005`.
