# TK-1 — сборка Rust / SQLite / FTS5 / vec1

Дата: 2026-10-21 (Europe/Moscow). Результат: **PASS / done** для согласованной матрицы macOS ARM64 и Linux ARM64 в Docker. Ветка `TK-1`, отдельный worktree `/Users/zdkadiev/.codex/worktrees/20f1/letopis`; исходный HEAD `2195c12372522b5c0b84ad5e785479930809f778`.

## Результат и границы

Создан сохраняемый пакет интеграционных проверок [tests/storage-build](../../../../../tests/storage-build/README.md). Он собирает нативный vec1, создаёт временные БД и проверяет выбранную связку без зависимости от старой Go-пробы или её корпуса. Это не production storage и не повтор исходных performance-измерений.

Закреплены Rust `1.95.0`, rusqlite `0.40.2`, libsqlite3-sys `0.38.2` / SQLite `3.53.2`, vec1 `0.8` из commit `a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7`. SHA-256 vec1 C: `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`. Заголовки берутся из того же libsqlite3-sys, что собирает SQLite; все Cargo-зависимости закреплены lockfile. Нативная библиотека собирается отдельно для каждой ОС в уникальном временном каталоге, источник проверяется до компиляции и атомарно публикуется в локальном кэше.

| Проверка | macOS ARM64 | Linux ARM64 |
|---|---|---|
| Rust target | aarch64-apple-darwin | aarch64-unknown-linux-gnu |
| C toolchain | Apple Clang 21.0.0 | GCC 12.2.0, Debian bookworm |
| SQLite / vec1 | 3.53.2 / 0.8, NEON | 3.53.2 / 0.8, NEON |
| Rust integration suite | 10 passed, 0 failed/skipped | 10 passed, 0 failed/skipped |
| Python source-integrity/publication suite | 4 passed | 4 passed |
| WAL / FULL / foreign_keys | PASS | PASS |
| macOS fullfsync | PASS | неприменимо |
| FTS5 contentless-delete read/search/delete/reopen | PASS | PASS |
| FTS5 insert/update/delete rollback, explicit и RAII | PASS | PASS |
| vec1 no-index/cos read/search/update/delete/reopen | PASS | PASS |
| vec1 insert/update/delete rollback, explicit и RAII | PASS | PASS |
| Совместные FTS/vec1 commit и rollback в одной БД | PASS | PASS |
| Несовпадение размерности вектора | отклонено, исходные строки сохранены | то же |
| Загрузка расширений после регистрации | запрещена | запрещена |
| Отсутствующая / некорректная vec1-библиотека | exit 1, явная ошибка | exit 1, явная ошибка |
| Реальная bundled-сборка без FTS5 (`-USQLITE_ENABLE_FTS5`) | exit 1, явная ошибка | exit 1, явная ошибка |
| Политика версии 3.51.2 / 3.51.3 | rejected / accepted | rejected / accepted |
| Реальная старая системная SQLite 3.40.1 | не запускалась | exit 1, `SQLite 3.51.3+ required` |
| `cargo fmt -- --check` | PASS | PASS |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS | PASS |

## Команды и evidence

Из корня репозитория:

```sh
python3 -B tests/storage-build/run.py --evidence docs/zkit/letopis/checks/TK-1/macos-arm64
docker build --platform linux/arm64 -t letopis-tk1:rust-1.95.0 tests/storage-build
docker run --rm --platform linux/arm64 \
  --mount "type=bind,source=$PWD/tests/storage-build,target=/checks" \
  --mount "type=bind,source=$PWD/docs/zkit/letopis/checks/TK-1,target=/evidence" \
  letopis-tk1:rust-1.95.0 \
  python3 -B run.py --evidence /evidence/linux-arm64 --system-sqlite
python3 -B docs/zkit/letopis/roadmap/schedule.py --check
```

Каталоги evidence должны отсутствовать перед запуском. Повторные прогоны сохраняются в новые каталоги, не перезаписывая результаты.

- [macOS manifest](macos-arm64/manifest.json): 13 команд; [Rust suite](macos-arm64/07-cargo.txt), [Python suite](macos-arm64/01-python3.txt).
- [Linux manifest](linux-arm64/manifest.json): 14 команд; [Rust suite](linux-arm64/07-cargo.txt), [Python suite](linux-arm64/01-python3.txt).
- Манифесты содержат hashes текущих файлов пакета, SQLite-заголовка и собранной vec1-библиотеки, точные argv, выбранные env, exit codes и полные журналы. После финальных запусков hashes сверены с текущими файлами.
- [RED политики подключения](red-policy.txt): первоначальный stub ошибочно принимал старую SQLite и отсутствующее расширение, не устанавливал WAL; все три теста это обнаружили.
- [Mutation RED](mutation-wal-red.txt) / [GREEN](mutation-wal-green.txt): замена WAL на DELETE вызвала ожидаемое падение; файл восстановлен byte-for-byte, все 10 Rust-тестов затем прошли.
- [RED публикации источника](source-publication-red.txt): читателю была видна частично записанная `.build/vec1.c`; исправлено атомарной заменой, регрессия входит в обе финальные Python suites.
- [Независимое ревью](review.md): единственное материальное замечание исправлено и проверено RED→GREEN.

Первый пробный запуск runner остановился на смешении Cargo metadata stdout с диагностикой загрузок в stderr. Потоки разделены; финальные запуски, включая Linux с пустым registry cache, прошли. Промежуточные журналы сохранены в ignored `.build/intermediate-evidence/`; PASS относится только к финальным каталогам выше.

## Сохранность и следующие задачи

Текущие незакоммиченные документы исходного checkout были скопированы и сверены по SHA-256 до работы. Исходный checkout не изменялся. В worktree из этого baseline изменены только карточка TK-1 и её статус в roadmap/EP-8; оценки, зависимости и недельная раскладка сохранены. Новые артефакты принадлежат TK-1. После первоначальной проверки изменения оставались незакоммиченными. По последующему запросу пользователя TK-1 коммитится и интегрируется отдельно от скопированных незакоммиченных правок baseline; эти правки сохраняются у пользователя.

TK-1 закрыта; TK-2…TK-45 остаются planned, G-0 ещё не закрыт. Следующая задача по расписанию — TK-2. ARM64 Docker — реальный Linux-процесс в Linux VM, не cross-compilation; x86-64, физический Linux-хост, production storage, ATTACH/config reset/старые read views (TK-3), нагрузка, providers и power-loss не проверялись. Соответствие NFR-5/NFR-7 всего продукта этим отчётом не объявляется.
