# Независимое ревью TK-2

2026-10-21. Отдельный reviewer `/root/review_tk2`, модель `gpt-6-astra`, high reasoning, чистый контекст. Ревью read-only: явные файлы `tests/publication/`, утверждённые plan/spec, evidence и решения ledger; unrelated copied design baseline исключён. Reviewer не менял checkout, index или HEAD.

## Результат reviewer

Critical: **0**. Important: **1**. Minor: **0**. Исходный вердикт: **With fixes**.

Reviewer подтвердил реальность child processes/SIGKILL/flock и границ транзакций, независимость before/after oracles, совпадение обоих финальных manifests с исходниками на момент ревью и содержательность четырёх мутаций.

**Important — `same_tick_same_size_edit_is_hashed` зависел от скорости setup.** Timestamp файла фиксировался до первичной инициализации/публикации; тест не устанавливал, что recorded checked_at остаётся в одном секундном тике с mtime. Задержка более секунды делала файл non-racy, и тест мог ошибочно обвинять корректный детектор. Reviewer воспроизвёл это в отдельной временной копии: при задержке 1.2 сек оставался старый текст, разность checked_at−mtime составляла 1,225,537,825 ns.

## Исправление исполнителя

Причина подтверждена чтением detector guard и теста. [RED reproduction](review-racy-window-red.log): реальному файлу заранее задан старый mtime, что детерминированно воспроизводит setup за пределами тика без sleep; прежняя последовательность оставляет `alpha` вместо ожидаемого `omega`.

Тест теперь явно нормализует **только логическое checked_at этой scratch fixture** к её mtime и проверяет точное равенство и racy-window условие. Затем сохраняет прежние проверки равенства реальных size/mtime/inode и нового FTS-результата. Поведение детектора не ослаблено. [GREEN full suite](review-racy-window-green.log): все 15 integration tests проходят; повторная racy-мутация по-прежнему обнаруживается. Обе платформенные suites и source manifests обновлены после исправления.

Незакрытых Critical/Important и отложенных Minor нет. Исправление проверено RED→GREEN и полными suites; повторное ревью не объявляется выполненным.

## Что reviewer не оценивал и решение исполнителя

- Полные Markdown/config/JCS, source identities и полнота graph API: утверждённая узкая fixture grammar сохраняется; production соответствие не заявляется.
- Corrupt-cache reconstruction/schema migration: reindex сохраняет last-valid invalid projections по MC-1; восстановление повреждённого кэша остаётся TK-26.
- Vector old readers/dimensions/drain/stale results: остаются TK-3, TK-2 доказывает только reset transaction boundary и identity recheck.
- Произвольные concurrent in-place writes и намеренное восстановление old non-racy mtime: прежние явные границы остаются; для полного атомарного чтения live corpus доказательства нет.
- Physical power loss/performance/unsupported architectures: не заявляются; TK-38 и последующие gates остаются обязательными.
- Финальные report/status wording: завершает исполнитель; test counts, cuts, source hashes и сохранность baseline сверяются отдельно в [final verification](final-verification.json).
