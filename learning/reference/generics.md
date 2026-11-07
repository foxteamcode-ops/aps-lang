# Справочник: Обобщённое программирование в APS (Generics Reference)

Обобщения (Generics) в **APS** позволяют писать параметризованный повторно используемый код для структур, перечислений, функций и методов с нулевыми накладными расходами времени выполнения (Zero-Cost Abstractions) благодаря полной мономорфизации.

---

## 1. Обобщённые функции

```aps
pub fn swap<T>(a: write T, b: write T) {
    let temp: T = take *a;
    *a = take *b;
    *b = take temp;
}

pub fn max<T: Comparable>(a: T, b: T) -> T {
    if a > b { a } else { b }
}
```

---

## 2. Обобщённые структуры и методы

```aps
pub struct KeyValue<K, V> {
    pub key: K,
    pub value: V,
}

impl<K, V> KeyValue<K, V> {
    pub fn new(key: K, value: V) -> KeyValue<K, V> {
        KeyValue { key, value }
    }

    pub fn get_key(read self) -> read K {
        read self.key
    }

    pub fn get_value(read self) -> read V {
        read self.value
    }
}
```

---

## 3. Трейты и ограничения типов (Traits & Bounds)

Трейты определяют контракты поведения, которые тип обязуется реализовать:

```aps
pub trait Printable {
    fn to_string(read self) -> String;
}

pub trait Hashable {
    fn hash(read self) -> u64;
}

// Ограничение с множественными трейтами (Multiple Bounds)
pub fn print_and_hash<T: Printable + Hashable>(item: read T) {
    print("Строка: " + item.to_string());
    print("Хеш: " + item.hash().to_string());
}
```

---

## 4. Мономорфизация (Monomorphization)

1. **Принцип работы:** при компиляции компилятор APS для каждой конкретной комбинации конкретных типов генерирует отдельную специализированную реализацию машинного кода на уровне LLVM IR.
2. **Нулевая стоимость абстракций:** вызовы методов обобщённых типов не используют виртуальные таблицы (vtable lookup) или динамическую диспетчеризацию, а транслируются в прямые машинные инструкции `call` или встраиваются (inlined).
3. **Контроль размера бинарного файла:** оптимизатор APS объединяет идентичные мономорфизации для указателей равного размера, предотвращая избыточное раздувание бинарного файла (Code Bloat).
