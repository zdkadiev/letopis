# TK-6 — независимое ревью и исправления

Reviewer: `/root/review_tk6_final`, gpt-6-astra/high, fresh context.
Проверял реальные untracked файлы: по принятому плану код остаётся без commit.
Одна полная проверка, затем один проход исправлений RED→GREEN; повторного
ревью нет. Снимок проверенных исходников и исторические PASS receipts:
[before-review-fix](before-review-fix/review-source-sha256.json).

## Полученный отчёт reviewer

Verdict: **Important fixes required before TK-6/G-0 completion.** Two findings;
no Critical findings. Reviewed the actual untracked implementation, tests,
fixtures, runners, Dockerfile, contract/spec/plan, report and final receipts.
No files changed.

1. **Important — cancelled JSON can exit successfully.**
   `tests/transport/src/bin/transport_client.rs:118–124`, interacting with `:103–104`.

   The signal thread writes cancelled, releases the output mutex, then calls
   exit(1). If the HTTP result completes concurrently, main can acquire the
   mutex, suppress its success JSON, and reach exit(0) before the signal
   thread reaches exit(1). Callers consequently see cancellation with a
   successful process status.

   Reproduced on the current, unchanged binary: a loop over the existing
   normal fixture, sending SIGINT after request acceptance with 0/20/40/60 μs
   scheduling offsets, produced on iteration 11:

   ```text
   {"error":{"code":"cancelled","details":{},"message":"cancelled"},"ok":false}
   exit_code = 0
   ```

   The existing test_sigint_response_race_has_one_complete_outcome also
   failed on repetition 20 with AssertionError: 0 != 1.

   Executor disposition: fix in the authorized Important RED→GREEN pass.
   Coordinate the winning output and process status; main must never exit 0
   after cancellation has won the output. Add deterministic coverage for
   this interleaving, then required green suites and fresh receipts.

2. **Important — race oracle rejects an explicitly permitted outcome.**
   `tests/transport/test_transport.py:124`.

   It requires exit 0 whenever the JSON is successful. D-96 explicitly
   permits preserving a previously printed success while a subsequent
   handled SIGINT exits 1; test_sigint_after_output_never_prints_second_json
   correctly tests that same behavior. The race oracle can therefore fail
   a correct implementation depending on scheduling.

   Observed on the unchanged binary: the same probe produced successful
   JSON with exit 1 twice. This is a valid outcome, distinct from finding 1.

   Executor disposition: correct the oracle in the same fix pass.
   Successful JSON may accompany exit 0 or 1; cancelled JSON must accompany
   exit 1. Continue requiring exactly one complete JSON, expected
   cancellation code, and no stderr.

Verified evidence:

- Review snapshot SHA256 map matches source bytes.
- Both platform manifests and all recorded evidence hashes verify.
- Negative controls: macOS 9, Linux 10; compiled, detected and restored.
- Ten TK-1…TK-5 manifests match committed bytes and current covered hashes.
- Separate macOS TLS probe binary hash matches the binary.
- Existing platform PASS receipts remain valid historical observations,
  but the newly reproduced race prevents treating them as sufficient
  completion evidence.

Declined to judge / executor boundaries:

- CPU parsing deadline and oversized-body memory exhaustion: expressly
  excluded and accurately documented. Keep the limitation; do not
  introduce body limits or a compute executor in this fix.
- Blocked stdout/backpressure and broader cancellation during SQL/flock/git:
  bounded fixture output does not establish these product guarantees.
  Keep TK-29 ownership; do not expand TK-6 into production CLI cancellation.
- Remote compute cancellation: process exit only ends local waiting; no
  contrary promise found. No change needed.
- Real inference, model quality, arbitrary compatible-server conformance,
  retry/storage/FTS behavior: explicitly outside this synthetic boundary
  proof. Preserve TK-13/TK-27/TK-41 ownership.
- macOS corporate CA installation and x86_64 support: accurately unproved.
  Linux custom-system-CA test plus WebPki mutation establish the chosen
  distinction there; public macOS HTTPS is narrower. Preserve those labels.
- Additional config restrictions, integer/f32 range refinements, duplicate
  JSON members and native response-model equality: no demonstrated required
  behavior beyond the accepted contract. Do not silently tighten the schema.
- URL, credentials, error formatting, redirects and proxies: reviewed
  without another actionable defect. Explicit URL construction, fixed
  error classes/no source chain, redacted prepared-request Debug and real
  trap endpoints support the claims.
- Network isolation: --network none invocation plus code inspection
  supports local synthetic transport proof, not a real-model offline drill.
- Historical FAIL receipts, unfinished status text and unchecked final-plan
  items: intentionally retained or pending review. Update status only after
  fixes and fresh verification; do not rewrite historical evidence.
- Full Letopis architecture: expressly declined by the user and outside
  this review. No redesign recommendation.

No second reviewer is needed under the accepted executor rules.

## Решение исполнителя

Обе находки остаются Important по эффекту для вызывающего агента; обе
исправлены в одном проходе, без нового runtime или HTTP worker.

- `Output::write_once` сообщает, выиграл ли вызывающий право на вывод.
  Main, проигравший уже напечатанному cancelled, завершает процесс с exit 1.
- Fixture-only gate удерживает поток сигнала после печати cancelled до exit;
  main уже получил успешный HTTP-ответ и удерживается перед выводом; затем fixture разрешает ему продолжить после cancelled. Это
  детерминированно воспроизводит найденное чередование.
- Общий race oracle проверен и после напечатанного success: exit 1
  допустим, повторного JSON и stderr нет. Для cancelled exit 1 обязателен.
- Добавлен compiling negative control `cancelled-exit`, убирающий именно
  защиту кода выхода. Исторические 9/10 controls остаются в before-review-fix;
  окончательные прогоны должны обнаружить 10/11 controls.

[Finding 1 RED](tdd/review-cancelled-exit-red.log) →
[GREEN](tdd/review-cancelled-exit-green.log),
[Finding 2 RED](tdd/review-race-oracle-red.log) →
[GREEN](tdd/review-race-oracle-green.log).
Первый вариант детерминированного теста удерживал незавершённый HTTP: на Linux это не гарантировало успешное завершение main после сигнала, и cancelled-exit mutation не была DETECTED. Runner остановился с FAIL; receipts сохранены в before-deterministic-gate. Gate перенесён после завершения HTTP; readiness явно подтверждает success перед SIGINT. Окончательные полные прогоны усиленного варианта PASS на обеих ОС: 8 Rust + 19 Python методов, 10/11 compiling controls DETECTED, включая cancelled-exit. Хэши текущих исходников и всех receipts проверены.

Все десять boundaries из Declined to judge приняты как ограничения
доказательства, с явным владельцем/обоснованием в progress ledger и отчёте.
Они не объявлены реализованными. Дополнительного ограничения JSON/URL/config
или продуктового механизма отмены не вводилось. Deferred minors: none.
