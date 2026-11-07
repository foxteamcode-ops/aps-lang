# Справочник: Структуры в APS (Structs Reference)

Структуры (`struct`) в **APS** представляют собой именованные составные типы (Product Types), объединяющие поля данных с детерминированным представлением в памяти без скрытых заголовков виртуальных таблиц.

---

## 1. Объявление структуры

```aps
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    max_connections: usize, // Приватное поле для текущего модуля
    pub enable_tls: bool,
}
```

### 1.1. Кортежные структуры (Tuple Structs)
```aps
pub struct Point3D(pub f64, pub f64, pub f64);
pub struct ProcessId(pub u32); // Паттерн Newtype для типобезопасности
```

### 1.2. Структуры-метки нулевого размера (Unit Structs)
Не занимают памяти в рантайме ($0$ байт) и используются для статических маркеров состояний:
```aps
pub struct EmptyToken;
```

---

## 2. Инициализация и создание экземпляров

```aps
// Стандартная инициализация
let config = ServerConfig {
    host: "127.0.0.1",
    port: 8080,
    max_connections: 10000,
    enable_tls: true,
};

// Field Init Shorthand (когда имя переменной совпадает с именем поля)
let port: u16 = 443;
let enable_tls = true;
let https_cfg = ServerConfig {
    host: "0.0.0.0",
    port,
    max_connections: 5000,
    enable_tls,
};
```

---

## 3. Модель памяти и раздельное заимствование (Disjoint Field Borrowing)

В отличие от языков, которые блокируют всю структуру целиком при заимствовании одного поля, анализатор **AMA (APS Adaptive Memory Architecture)** поддерживает гранулированное отслеживание полей (Field-Sensitive Aliasing Analysis).

### Пример безопасного раздельного заимствования полей:
```aps
struct Pair {
    first: Vector<i64>,
    second: Vector<i64>,
}

fn process_pair(pair: write Pair) {
    // В APS разрешено одновременно модифицировать pair.first
    // и читать pair.second, так как поля не перекрываются в памяти!
    let r2 = read pair.second;
    let w1 = write pair.first;
    w1.push(42);
    print(r2.len()); // Безопасно, ошибки E0302 нет!
}
```

Если же попытаться одновременно получить `write pair.first` и `read pair.first`, компилятор сгенерирует диагностику:
```
error[E0302]: conflicting capabilities on field 'first': cannot borrow as 'write' while borrowed as 'read'
```

---

## 4. Память и выравнивание (Alignment)

Структуры в APS упорядочиваются компилятором с автоматическим паддингом для соблюдения естественного выравнивания полей:
- Поля `u64` / `i64` выравниваются по 8 байтам.
- Поля `u32` / `i32` — по 4 байтам.
- Поля `u16` — по 2 байтам.
- Поля `u8` / `bool` — по 1 байту.
- Общий размер структуры кратен максимальному выравниванию её полей.
