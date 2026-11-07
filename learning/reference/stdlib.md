# Справочник: Стандартная библиотека APS (Standard Library Reference)

Стандартная библиотека **APS** состоит из 17 модулей, спроектированных по принципу нулевых скрытых аллокаций и строгой безопасности памяти AMA.

---

## 1. Карта модулей стандартной библиотеки

| Модуль | Описание | Ключевые типы и функции |
|---|---|---|
| `std::core` | Фундаментальные примитивы языка | `Option`, `Result`, `panic`, `assert`, `size_of`, `align_of` |
| `std::memory` | Управление памятью, аллокаторы, арены | `RegionArena`, `Box`, `ManuallyDrop`, `Layout` |
| `std::string` | Управление динамическими UTF-8 строками | `String`, `StringBuilder`, `Utf8Error`, `from_utf8` |
| `std::collections` | Высокопроизводительные структуры данных | `Vector`, `HashMap`, `HashSet`, `RingBuffer`, `LinkedList` |
| `std::math` | Математика, тригонометрия, спецфункции | `sin`, `cos`, `sqrt`, `pow`, `abs`, константы `PI`, `E`, `TAU` |
| `std::io` | Потоки ввода-вывода, буферизация | `Read`, `Write`, `BufReader`, `BufWriter`, `stdin`, `stdout` |
| `std::fs` | Файловая система и манипуляции файлами | `File`, `OpenOptions`, `read_to_string`, `write_all`, `Metadata` |
| `std::net` | Сетевой стек TCP / UDP | `TcpListener`, `TcpStream`, `UdpSocket`, `SocketAddr`, `IpAddr` |
| `std::time` | Время, интервалы, монотонные часы | `Instant`, `Duration`, `SystemTime`, `sleep` |
| `std::process` | Управление процессами ОС и аргументами | `Command`, `Child`, `args`, `env`, `exit`, `pid` |
| `std::thread` | Нативные потоки операционной системы | `Thread`, `spawn`, `join`, `yield_now`, `sleep`, `current` |
| `std::sync` | Примитивы синхронизации и каналы | `Mutex`, `RwLock`, `channel`, `Condvar`, `Barrier`, `atomic::*` |
| `std::async` | Рантайм асинхронных задач и реактор | `Future`, `Task`, `Waker`, `join`, `select`, `block_on` |
| `std::crypto` | Криптографические хеш-функции | `sha256`, `sha512`, `hmac_sha256`, `keccak256`, `constant_time_eq` |
| `std::encoding` | Кодирование и сериализация форматов | `Base64`, `Hex`, `JsonValue`, `JsonParser`, `Utf8` |
| `std::testing` | Встроенный тестовый фреймворк | `assert_eq`, `assert_ne`, `assert_true`, `run_tests` |
| `std::platform` | Низкоуровневые интерфейсы ОС | Архитектура CPU, размер страниц памяти, POSIX/Win32 syscalls |

---

## 2. Примеры подключения и использования

```aps
// Подключение конкретных типов
import std::collections::Vector;
import std::fs::File;
import std::time::Instant;

fn main() {
    let timer = Instant::now();
    let mut data = Vector::with_capacity(1024);
    data.push(100);

    print("Прошло времени: " + timer.elapsed_millis().to_string() + " мс");
}
```
