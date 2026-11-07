# Справочник: Управление памятью и структура данных в APS (Memory Reference)

Язык **APS** ориентирован на максимальный контроль над памятью вычислительного узла, обеспечивая плотную компоновку структур в кэш-линиях процессора (Cache Efficiency) и предсказуемую стоимость любых манипуляций.

---

## 1. Стековая и кучевая память

| Характеристика | Стек (Stack) | Куча (Heap) | Арена (Region Arena) |
|---|---|---|---|
| **Скорость аллокации** | $< 1$ нс (1 процессорная инструкция `sub rsp, N`) | $20 - 150$ нс (поиск блока, блокировки аллокатора) | $\approx 2$ нс (bump-инкремент `ptr += size`) |
| **Скорость деаллокации** | $< 1$ нс (`add rsp, N`) | $30 - 200$ нс (слияние блоков, возврат в пул) | $\approx 2$ нс (обнуление `used = 0` для всей арены) |
| **Фрагментация** | 0% | Возможна при случайных размерах | 0% внутри арены |
| **Потокобезопасность** | Локально для каждого потока ОС | Синхронизируется мьютексами аллокатора | Локальна для блока `region` |

---

## 2. Представление указателей в памяти

1. **Тонкий указатель (Thin Pointer, 8 байт):**
   - Используется для ссылок `read T` и `write T` на типы фиксированного размера (`Sized`).
   - Представляет собой 64-битный виртуальный адрес в памяти.
2. **Толстый указатель (Fat Pointer, 16 байт):**
   - Используется для срезов `&[T]` и строковых срезов `&str`.
   - Содержит два 64-битных поля: `(data_ptr: *const T, len: usize)`.
3. **Управляемый вектор (Managed Vector, 24 байта):**
   - Представляет тип `Vector<T>`.
   - Содержит три поля: `(data_ptr: *mut T, capacity: usize, len: usize)`.

---

## 3. Архитектура Region Bump Allocator

В рантайме APS (`runtime/aps_runtime.c`) арена памяти реализована через легковесные чанки фиксированного размера (по умолчанию 64 КБ):

```c
typedef struct ApsRegionChunk {
    struct ApsRegionChunk* next;
    size_t capacity;
    size_t used;
    uint8_t data[];
} ApsRegionChunk;
```

### Алгоритм аллокации $O(1)$:
```c
void* aps_region_alloc(ApsRegion* r, size_t size, size_t align) {
    size_t aligned_used = (r->current->used + (align - 1)) & ~(align - 1);
    if (aligned_used + size <= r->current->capacity) {
        void* ptr = r->current->data + aligned_used;
        r->current->used = aligned_used + size;
        return ptr;
    }
    // Выделение нового чанка только при переполнении текущего
    return aps_region_alloc_new_chunk(r, size);
}
```

При выходе из блока `region` вызывается `aps_region_destroy()`, которая освобождает цепочку связанных чанков за один проход, мгновенно ликвидируя миллионы временных объектов.

---

## 4. Защита от переполнения буфера (Bounds Checking)

При индексации массивов компилятор внедряет проверку:
```c
void aps_bounds_check(size_t index, size_t len, const char* file, int line) {
    if (index >= len) {
        fprintf(stderr, "APS Panic [R0001]: Index %zu out of bounds for length %zu at %s:%d\n",
                index, len, file, line);
        abort();
    }
}
```
Это полностью устраняет уязвимости переполнения буфера в безопасном коде APS.
