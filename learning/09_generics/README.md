# Глава 09: Обобщённое программирование и Трейты в APS

Обобщения (Generics) позволяют писать универсальный, типобезопасный код без дублирования логики и без замедления работы программ.

---

## 1. Концепция нулевой стоимости (Zero-Cost Generics)

В отличие от языков с виртуальной машиной (Java, C#), где обобщения приводят к боксингу примитивов (`Integer`) или стиранию типов (Type Erasure), APS использует **полную статическую мономорфизацию**:

```aps
fn print_item<T: Printable>(item: read T) {
    print(item.to_string());
}
```

Компилятор APS при трансляции в машинный код генерирует отдельные эффективные функции для `print_item<i64>`, `print_item<String>` и т.д. В результате скорость выполнения обобщённого кода абсолютно идентична коду, написанному вручную для каждого конкретного типа!

---

## 2. Обобщённые структуры данных

```aps
pub struct Queue<T> {
    elements: Vector<T>,
}

impl<T> Queue<T> {
    pub fn new() -> Queue<T> {
        Queue { elements: Vector::new() }
    }

    pub fn enqueue(write self, item: T) {
        self.elements.push(item);
    }

    pub fn is_empty(read self) -> bool {
        self.elements.len() == 0
    }
}
```

---

## 3. Трейты как интерфейсные контракты

Трейт (`trait`) определяет набор сигнатур методов, которые тип обязан предоставить:

```aps
pub trait Serialize {
    fn serialize(read self) -> Vector<u8>;
}

pub trait Deserialize {
    fn deserialize(bytes: read [u8]) -> Result<Self, String>;
}

// Реализация трейта для пользовательской структуры:
struct UserRecord {
    id: u64,
    name: String,
}

impl Serialize for UserRecord {
    fn serialize(read self) -> Vector<u8> {
        var buf = Vector::new();
        // Бинарная упаковка полей...
        buf
    }
}
```

---

## 4. Множественные ограничения типов (Multiple Trait Bounds)

```aps
fn backup_and_verify<T: Serialize + Clone + Equatable>(item: read T) {
    let copy = item.clone();
    let payload = item.serialize();
    // Проверка инвариантов...
}
```
Если передаваемый тип не удовлетворяет хотя бы одному из указанных ограничений, компилятор останавливает сборку с точной диагностикой:
```
error[E0404]: type 'RawConnection' does not implement trait 'Serialize'
```
