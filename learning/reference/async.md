# Справочник: Асинхронное программирование в APS (Async Reference)

Асинхронная модель **APS** базируется на безстековых корутинах (Stackless Coroutines), транслируемых компилятором в компактные конечные автоматы (State Machines) на уровне LLVM IR, и высокопроизводительном реакторе событий ввода-вывода (IOCP на Windows, epoll на Linux, kqueue на macOS).

---

## 1. Синтаксис `async` и `await`

```aps
import std::net::TcpStream;

async fn fetch_response(host: String, port: u16) -> Result<String, Error> {
    let mut stream = TcpStream::connect_async(host, port).await?;
    stream.write_all_async(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await?;
    
    var response_buf = Vector::with_capacity(4096);
    stream.read_to_end_async(write response_buf).await?;
    
    Ok(String::from_utf8(response_buf)?)
}
```

---

## 2. Безстековые корутины и размер фрейма

1. Каждая функция с модификатором `async` возвращает анонимный тип, реализующий трейт `Future<Output = T>`.
2. Состояние локальных переменных упаковывается компилятором в сгенерированную структуру-фрейм задачи.
3. Размер структуры точно рассчитывается во время компиляции. Если переменная не пересекает точку вызова `.await`, она остаётся в регистрах процессора и не занимает памяти во фрейме корутины.

---

## 3. Параллельное выполнение: `join` и `select`

```aps
import std::async::{join, select};

async fn handle_dashboard() {
    // Параллельный запуск двух асинхронных операций
    let (user_profile, user_orders) = join(
        fetch_profile(user_id),
        fetch_orders(user_id)
    ).await;

    // Гонка с тайм-аутом (Timeout pattern)
    match select(fetch_data(), timeout(Duration::from_secs(2))).await {
        First(data) => print("Успешно получены данные: " + data),
        Second(_) => print("Ошибка: Превышен таймаут ответа сервера"),
    }
}
```
