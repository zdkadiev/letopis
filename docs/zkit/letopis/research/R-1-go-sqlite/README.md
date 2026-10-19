# Исследовательские пробы Go и SQLite

Это временные диагностические программы для исследования от 12 октября 2026 года, не реализация Engram. Выводы и ограничения описаны в [отчёте](../R-1-go-sqlite.md).

Состав:

- `probe/main.go`: vec1, FTS5, несколько соединений, откат, WAL-снимок, фильтры, 50 000 синтетических векторов, отмена.
- `probe/ncruces.go`: регистрация Go-версий FTS5 и vec1.
- `probe/native.go`: mattn и нативное расширение, тег `native`; `ENGRAM_PROBE_EXTENSION` задаёт только путь расширения исследовательской программы.
- `modernc/main.go`: минимальная проверка другого расширения sqlite-vec.
- `go.mod`, `go.sum`: закреплённые зависимости пробы.
- `*-results.txt`: полные выводы финальных измерений, `build-manifest.json`: версии и SHA-256 исходника C.

Программы создают свои тестовые БД. Для vec1 нужно указывать отсутствующий путь; при повторном запуске используйте новое имя. modernc-проба использует фиксированный временный путь из исходника, поэтому при воспроизведении замените его на свободный путь. Не запускайте эти программы на БД проекта.

Для воспроизведения скопируйте эту папку во временную директорию и выполняйте команды из неё. Ncruces:

```sh
CGO_ENABLED=0 go build -o ncruces-probe ./probe
./ncruces-probe /private/tmp/engram-vec1-repeat.db 50000
```

Нативный вариант на macOS ARM64: загрузите `vec1.c` из закреплённого commit, проверьте SHA-256 и используйте согласованные заголовки. Следующие команды соответствуют способу выполненной сборки; строка `CGO_ENABLED=1` включает требуемый драйверу CGO.

```sh
curl -fsSL https://raw.githubusercontent.com/sqlite/sqlite/a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7/ext/vec1/vec1.c -o vec1.c
shasum -a 256 vec1.c
go mod download github.com/mattn/go-sqlite3@v1.14.52
go list -m -f '{{.Dir}}' github.com/mattn/go-sqlite3
```

В найденной директории находятся `sqlite3-binding.h` и `sqlite3ext.h`. Скопируйте их в локальную `include/`, а также сделайте копию `sqlite3-binding.h` с именем `sqlite3.h`. Не меняйте файлы кэша Go-модулей.

```sh
cc -O3 -DNDEBUG -I include vec1.c -shared -fPIC -o vec1.dylib
CGO_ENABLED=1 go build -tags 'native sqlite_fts5' -o native-probe ./probe
ENGRAM_PROBE_EXTENSION="$PWD/vec1.dylib" ./native-probe /private/tmp/engram-native-repeat.db 50000
```

На Linux нужны собственная сборка `.so` и C-toolchain; соответствующая проверка запуска ещё не выполнена. AVX2/FMA нельзя безусловно требовать от всех x86-процессоров.

Modernc:

```sh
CGO_ENABLED=0 go run ./modernc
```

Проверка сборки ncruces для Linux выполнена следующими командами. Она не заменяет запуск на Linux.

```sh
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -o ncruces-linux-amd64 ./probe
GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go build -o ncruces-linux-arm64 ./probe
```

Точные времена зависят от системы и фоновой нагрузки. Контексты с deadline 1 мс используются только как диагностический способ вызвать отмену; продуктовые ограничения времени запроса этим не задаются.
