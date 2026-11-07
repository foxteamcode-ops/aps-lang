# Справочник: Функции в APS (Functions Reference)

Функции являются фундаментальными строительными блоками в **APS**. Они поддерживают статическую типизацию параметров, явные права доступа AMA (`read`, `write`, `take`), параметры по умолчанию, обобщения (Generics) и хвостовую рекурсию.

---

## 1. Базовый синтаксис объявления

```aps
fn function_name(param1: Type1, param2: Type2) -> ReturnType {
    // Тело функции
    // Последнее выражение без точки с запятой является возвращаемым значением
    param1 + param2
}
```

Если функция не возвращает значение, возвращаемый тип опускается (эквивалентно `-> ()`):
```aps
fn log_message(msg: String) {
    print("[LOG] " + msg);
}
```

---

## 2. Параметры и Access Capabilities в сигнатуре

В APS параметры функций явно объявляют свои намерения относительно владения и изменения памяти:

| Модификатор параметра | Семантика | Пример |
|---|---|---|
| `param: read T` | Чтение данных по ссылке без изменения. Исходный объект остаётся доступен вызывающей стороне. | `fn checksum(data: read Vector<u8>) -> u64` |
| `param: write T` | Эксклюзивный доступ на запись. Вызывающая сторона передаёт право на модификацию. | `fn append_log(buf: write Vector<String>, msg: String)` |
| `param: take T` | Полная передача владения (Move). Вызывающая сторона теряет доступ к объекту. | `fn send_to_channel(payload: take Packet)` |
| `param: T` (для Copy типов) | Передача по значению копированием байтов на стеке. | `fn square(x: i64) -> i64` |

---

## 3. Методы структур (`impl` блоки)

Методы определяются внутри блоков `impl Typename` и принимают в качестве первого параметра `self`:

```aps
struct Account {
    id: i64,
    balance: i64,
}

impl Account {
    // Статический конструктор (без self)
    pub fn new(id: i64, initial_balance: i64) -> Account {
        Account { id: id, balance: initial_balance }
    }

    // Метод только для чтения (read self)
    pub fn get_balance(read self) -> i64 {
        self.balance
    }

    // Метод модификации (write self)
    pub fn deposit(write self, amount: i64) {
        self.balance += amount;
    }

    // Метод поглощения (take self)
    pub fn close(take self) -> i64 {
        let final_balance = self.balance;
        // self деаллоцируется детерминированно
        final_balance
    }
}
```

---

## 4. Замыкания и функции высшего порядка (Closures)

Функции первого класса (First-Class Functions) могут передаваться как аргументы и возвращаться из функций:

```aps
// Тип функции: fn(i64) -> bool
fn filter_positive(items: read Vector<i64>, predicate: fn(i64) -> bool) -> Vector<i64> {
    var result = Vector::new();
    for x in items {
        if predicate(x) {
            result.push(x);
        }
    }
    result
}

// Использование лямбда-синтаксиса замыканий:
let evens = filter_positive(numbers, |x| x % 2 == 0);
```

---

## 5. Рекурсия и инлайнинг

1. **Tail Call Optimization (TCO):** функции с хвостовой рекурсией гарантированно разворачиваются компилятором APS в итеративный цикл на уровне LLVM IR.
2. **Атрибут инлайнинга:** Небольшие однострочные функции автоматически определяются оптимизатором APS как кандидаты на подстановку (`inlinehint`), исключая стоимость вызова `call/ret`.
