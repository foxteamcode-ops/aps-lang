# Глава 16: Сетевое программирование, Сокеты и Протоколы в APS

В этой главе рассматривается создание масштабируемых сетевых приложений на APS: TCP-серверов, UDP-клиентов, парсинг бинарных фреймов и реализация протокола HTTP/1.1 с нулевым копированием (Zero-Copy Networking).

---

## 1. TCP-сервер и сокеты: `std::net`

```aps
import std::net::{TcpListener, TcpStream};
import std::thread;

fn start_echo_server(port: u16) -> Result<(), NetworkError> {
    let listener = TcpListener::bind("0.0.0.0", port)?;
    print("Сервер запущен и слушает порт: " + port.to_string());

    loop {
        // Принятие входящего клиентского подключения
        let (stream, client_addr) = listener.accept()?;
        print("Подключился клиент с адреса: " + client_addr.to_string());

        // Обработка клиента в отдельном потоке или асинхронной задаче
        thread::spawn(|| {
            handle_client(stream);
        });
    }
}

fn handle_client(mut stream: TcpStream) {
    defer stream.close();
    var buffer: [u8; 1024] = [0u8; 1024];

    loop {
        let bytes = stream.read(write buffer).unwrap_or(0);
        if bytes == 0 {
            break; // Клиент отключился
        }
        // Эхо-ответ: отправляем полученные байты обратно
        let _ = stream.write_all(buffer[0..bytes]);
    }
}
```

---

## 2. Протокол без установления соединения: `UdpSocket`

Для потокового видео, телеметрии и высокочастотного гейминга модуль `std::net::UdpSocket` обеспечивает минимальную сетевую задержку без TCP Handshake:

```aps
import std::net::UdpSocket;

fn udp_heartbeat_sender() -> Result<(), NetworkError> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let target_addr = "192.168.1.100:8888";

    let heartbeat = b"PING_ALIVE";
    socket.send_to(heartbeat, target_addr)?;
    Ok(())
}
```

---

## 3. Парсинг протокола HTTP/1.1 с использованием срезов

В APS заголовки HTTP парсятся без создания сотен мелких объектов `String` в куче. Парсер работает со срезами `&str` исходного входного буфера:

```aps
struct HttpRequest<'a> {
    method: &'a str,
    path: &'a str,
    version: &'a str,
}

fn parse_request_line<'a>(line: &'a str) -> Option<HttpRequest<'a>> {
    let mut parts = line.split(" ");
    let method = parts.next()?;
    let path = parts.next()?;
    let version = parts.next()?;
    Some(HttpRequest { method, path, version })
}
```
Такой подход устраняет давление на память и позволяет серверу на APS обрабатывать свыше 150 000 HTTP-запросов в секунду на одном процессорном ядре!
