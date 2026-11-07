# Справочник: Коллекции в APS (Collections Reference)

Модуль `std::collections` предоставляет набор высокопроизводительных структур данных с предсказуемой амортизированной сложностью и полной поддержкой модели AMA.

---

## 1. Обзор основных коллекций

| Коллекция | Описание | Доступ по индексу | Вставка в конец | Поиск по ключу | Удаление |
|---|---|---|---|---|---|
| `Vector<T>` | Непрерывный динамический массив | $O(1)$ | $O(1)$ аморт. | $O(n)$ | $O(1)$ с конца, $O(n)$ из середины |
| `HashMap<K, V>` | Хеш-таблица с открытой адресацией (Robin Hood Hashing) | — | $O(1)$ аморт. | $O(1)$ в среднем | $O(1)$ в среднем |
| `HashSet<T>` | Множество уникальных элементов | — | $O(1)$ аморт. | $O(1)$ в среднем | $O(1)$ в среднем |
| `RingBuffer<T>` | Кольцевой буфер фиксированного размера для очередей задач | $O(1)$ | $O(1)$ | — | $O(1)$ с начала (FIFO) |

---

## 2. Использование `Vector<T>`

```aps
import std::collections::Vector;

fn example_vector() {
    var nums = Vector::new();
    nums.push(10);
    nums.push(20);
    nums.push(30);

    print("Размер: " + nums.len().to_string()); // 3
    print("Первый элемент: " + nums[0].to_string()); // 10

    // Итерация по ссылке (read)
    for n in nums.iter() {
        print(n);
    }

    // Извлечение с конца
    let last = nums.pop(); // Some(30)
}
```

---

## 3. Использование `HashMap<K, V>`

```aps
import std::collections::HashMap;

fn example_hashmap() {
    var scores = HashMap::new();
    scores.insert("Alice", 100);
    scores.insert("Bob", 85);

    match scores.get("Alice") {
        Some(score) => print("Баллы Алисы: " + score.to_string()),
        None => print("Игрок не найден"),
    }

    if scores.contains_key("Bob") {
        scores.remove("Bob");
    }
}
```

---

## 4. Оптимизация аллокаций: `with_capacity`

Для предотвращения частых перевыделений памяти при заполнении коллекций рекомендуется инициализировать их с заранее известной емкостью:

```aps
var fast_vec = Vector::with_capacity(100_000);
var fast_map = HashMap::with_capacity(50_000);
```
Это исключает накладные расходы на копирование буфера при росте массива.
