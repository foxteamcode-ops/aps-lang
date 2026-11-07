# Справочник: Многопоточность и Синхронизация в APS (Concurrency Reference)

Модель многопоточности **APS** строится на нативных системных потоках операционной системы в сочетании с безопасной моделью памяти **AMA**, гарантирующей отсутствие гонок данных (Data-Race Freedom) на уровне компиляции.

---

## 1. Запуск потоков (`std::thread` и `spawn`)

```aps
import std::thread;

fn example_thread() {
    let handle = thread::spawn(|| {
        print("Привет из параллельного потока!");
        42
    });

    let result = handle.join();
    print("Результат потока: " + result.to_string());
}
```

---

## 2. Каналы передачи сообщений (Message-Passing Channels)

В соответствии с философией системного программирования APS: *"Не разделяйте память для общения — общайтесь для разделения памяти"*.

Каналы передают владение объектами по семантике `take T`, что исключает необходимость блокировок при чтении переданных данных:

```aps
import std::sync::channel;

fn example_channel() {
    let (sender, receiver) = channel::bounded(100);

    spawn {
        let payload = Packet::new("sensors_data");
        // Передача владения в канал
        sender.send(take payload);
    };

    let received_packet = receiver.recv();
    print("Получен пакет: " + received_packet.name);
}
```

---

## 3. Примитивы синхронизации: `Mutex` и `Atomic`

### 3.1. Защита изменяемых данных: `Mutex<T>`
```aps
import std::sync::Mutex;

let counter = Mutex::new(0);

// Потокобезопасный инкремент через RAII Lock Guard
{
    let guard = counter.lock();
    *guard += 1;
} // guard автоматически освобождает блокировку в конце блока
```

### 3.2. Атомарные скаляры: `std::sync::atomic`
Для сверхбыстрых счётчиков без блокировок операционной системы:
- `AtomicBool`
- `AtomicI64`
- `AtomicUsize`

```aps
import std::sync::atomic::AtomicI64;

let global_id = AtomicI64::new(1);
let current = global_id.fetch_add(1, Ordering::SeqCst);
```
