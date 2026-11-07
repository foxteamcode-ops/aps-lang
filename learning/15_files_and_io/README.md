# Глава 15: Файловая система, Потоки ввода-вывода и Буферизация в APS

В этой главе рассматриваются низкоуровневые и высокоуровневые операции ввода-вывода (I/O) в APS: работа с файлами, каталогами, буферизированными потоками и реализация журнала Write-Ahead Log (WAL).

---

## 1. Открытие и чтение файлов: `std::fs::File`

```aps
import std::fs::File;

fn read_file_example() -> Result<(), IOError> {
    // 1. Быстрое чтение всего файла целиком
    let text = std::fs::read_to_string("config.toml")?;
    print("Содержимое конфига:\n" + text);

    // 2. Открытие файлового дескриптора
    let mut file = File::open("input.bin")?;
    defer file.close(); // Детерминированное закрытие при выходе

    var buffer: [u8; 4096] = [0u8; 4096];
    let bytes_read = file.read(write buffer)?;
    print("Прочитано байт: " + bytes_read.to_string());

    Ok(())
}
```

---

## 2. Буферизированный ввод-вывод (`BufReader` и `BufWriter`)

Каждый системный вызов ядра (`read`/`write` syscall) сопровождается переключением контекста из пространства пользователя в пространство ядра (User-to-Kernel Mode Switch), что занимает сотни наносекунд. Буферизация объединяет мелкие операции в блоки по 8–64 КБ:

```aps
import std::io::{BufReader, BufWriter};

fn process_large_file() -> Result<(), IOError> {
    let source = File::open("source.log")?;
    let mut reader = BufReader::new(source);

    let destination = File::create("filtered.log")?;
    let mut writer = BufWriter::new(destination);

    while let Some(line) = reader.read_line()? {
        if line.contains("ERROR") {
            writer.write_line(line)?;
        }
    }
    writer.flush()?; // Принудительный сброс остатка буфера на диск
    Ok(())
}
```

---

## 3. Реализация журнала с опережающей записью (Write-Ahead Log)

Для создания транзакционных баз данных и очередей сообщений файлы открываются с флагом прямого сброса на диск `sync`:

```aps
import std::fs::OpenOptions;

struct WalEntry {
    lsn: u64,
    payload: Vector<u8>,
}

impl WalEntry {
    pub fn append_to_wal(read self, file: write File) -> Result<(), IOError> {
        let binary_data = self.encode();
        file.write_all(binary_data)?;
        file.sync_data()?; // Аппаратный сброс кеша контроллера диска (fsync)
        Ok(())
    }
}
```
Это гарантирует сохранность данных даже при внезапном отключении электропитания сервера.
