# Глава 07: Структуры, Перечисления и Методы в APS

В этой главе мы рассмотрим создание пользовательских типов данных: структур (`struct`) для моделирования сущностей с именованными полями и перечислений (`enum`) для моделирования состояний и размеченных объединений (Tagged Unions).

---

## 1. Определение структур и создание экземпляров

Структуры в APS представляют собой непрерывные блоки памяти без скрытых служебных заголовков:

```aps
pub struct ServerNode {
    pub id: u64,
    pub ip_address: String,
    pub port: u16,
    is_active: bool, // Приватно для текущего модуля
}

fn create_node() -> ServerNode {
    ServerNode {
        id: 101,
        ip_address: "192.168.1.10".to_string(),
        port: 9000,
        is_active: true,
    }
}
```

---

## 2. Реализация методов в блоках `impl`

Методы привязываются к типам внутри блоков `impl`. В качестве первого аргумента они принимают `self` с явным указанием права доступа:

```aps
impl ServerNode {
    // 1. Статическая функция-конструктор (без self)
    pub fn new(id: u64, ip: String, port: u16) -> ServerNode {
        ServerNode {
            id,
            ip_address: ip,
            port,
            is_active: false,
        }
    }

    // 2. Метод только для чтения (read self)
    pub fn endpoint(read self) -> String {
        self.ip_address + ":" + self.port.to_string()
    }

    // 3. Метод для изменения состояния (write self)
    pub fn activate(write self) {
        self.is_active = true;
    }

    // 4. Метод поглощения структуры (take self)
    pub fn decommission(take self) {
        print("Вывод узла " + self.id.to_string() + " из эксплуатации...");
        // self автоматически деаллоцируется
    }
}
```

---

## 3. Перечисления с полезной нагрузкой (Sum Types)

В отличие от классических языков C/Java, варианты перечислений в APS могут содержать разнородные данные:

```aps
pub enum TaskStatus {
    Pending,
    Running { progress_percent: u8 },
    Completed(String), // Результат выполнения
    Failed { error_code: i32, message: String },
}
```

### Обработка состояний через исчерпывающий `match`:
```aps
fn print_status(status: read TaskStatus) {
    match status {
        TaskStatus::Pending => print("Ожидает в очереди"),
        TaskStatus::Running { progress_percent } => {
            print("Выполняется: " + progress_percent.to_string() + "%");
        }
        TaskStatus::Completed(result) => {
            print("Успешно завершено: " + result);
        }
        TaskStatus::Failed { error_code, message } => {
            print("Ошибка #" + error_code.to_string() + ": " + message);
        }
    }
}
```

---

## 4. Раздельное заимствование полей (Disjoint Field Borrowing)

В модели AMA компилятор отслеживает независимость полей структуры:
```aps
struct WorkerState {
    input_queue: Vector<Task>,
    output_log: Vector<String>,
}

fn step(state: write WorkerState) {
    // В APS разрешено одновременно читать input_queue и писать в output_log!
    let tasks = read state.input_queue;
    let log = write state.output_log;

    log.push("Задач в очереди: " + tasks.len().to_string());
}
```
Это делает код значительно чище, устраняя искусственные ограничения заимствования, присущие ранним версиям Rust.
