# Parser CPU experiment — line coordinates

Status: promising isolated candidate; coordinator must measure real full100k search serially. Root checkout and corpus are untouched. Changes exist only in this experiment's private clone.

Baseline revision: `3d820de41ee12385bc734ac4ceeb56b78ba379f9`.

## Finding and retained change

`src/markdown/scan.rs::layout_build` formerly called `line_number_calculate(text, at)` twice per element. Both calls counted every LF from byte zero to the requested coordinate. Valid section elements have already been proven ordered and non-overlapping by the preceding `NestedElement` check. Reuse that fact: carry the last counted byte offset and its line number, count the intervening gap to each section start and then that section's remaining bytes to its inclusive end coordinate. These adjacent counting spans visit each byte at most once across the complete parse.

The candidate uses two local integers and one private byte-count helper; it adds no allocation, new component, public API, dependency, runtime, parser option or permanent/request cache. It keeps the original complete `pulldown-cmark` parse, original heading and metadata collection, all link processing and search rendering, original byte and SHA-256 spans, and original diagnostics/control-flow order. Parsed data observation remains per parser invocation; no I/O or current-byte checks change. Layout validation rejects overlapping elements before advancing the cursor. A whole-file element still scans the same relevant byte prefix once.

Let `N(a,b)` count LF bytes in `[a,b)`. The stored invariant is `line_number = 1 + N(0,line_offset)`. The preceding overlap check guarantees `line_offset <= begin`; adding `N(line_offset,begin)` gives the exact first-line coordinate, and then adding `N(begin,end-1)` gives the original inclusive last-line coordinate. Setting `line_offset = end-1` reestablishes the invariant. The last LF of an element is therefore excluded from its own final-line count but included when locating the following element, including immediately adjacent sections.

The cursor intentionally counts byte slices. `end - 1` is an inclusive byte coordinate and may lie inside a final Unicode scalar when the document lacks a final newline. Slicing `str` there would panic; the first candidate failed the new Unicode test for exactly that mistake, and the retained candidate uses `&[u8]` consistently with the original implementation.

Unchanged candidate opportunities examined: heading bounds search repeats a heading scan, edge ownership repeats an element scan, and search rendering repeats the full event scan per element. These were not changed because the simplest line-count change already has a measured substantial effect and avoids adding range-search assumptions or state to unrelated parsing responsibilities.

## Profile context

Historical native follow-up `ollama/full-native-stacks.json` reports inclusive CPU deltas: `file_parse` 60,022,962 / process total 111,901,117; `layout_scan` 35,462,421; `edges_collect` 8,251,663; `search_text_render` 2,416,057. Inclusive costs overlap and do not identify exclusive time of the line-count helper. These are motivation, not new candidate measurements.

## Independent work count

The read-only real synthetic input is `ollama-100k/repository/knowledge/part-0000.md`: 60,000 bytes, 330 LF delimiters, 30 sections of exactly 2,000 bytes. Full baseline and candidate Debug projections are byte-identical.

- Baseline counting work is the sum of two prefix lengths per section: **1,799,970 bytes**.
- Candidate counting work spans byte zero through final inclusive coordinate once: **59,999 bytes**.
- Thus line-counter work falls **30 times** with **zero additional allocations**.

`work-counts.json` records input/projection SHA-256 and counts. This counts bytes presented to the exact newline counters; it does not claim all parsing passes are removed.

## Parser microbenchmark

`parser_cpu_probe.rs` reads the input outside the timed region, performs ten warm-up parses, and times 2,000 complete `markdown::file_parse` calls with `black_box` inputs/results. Both baseline and candidate were built with `--locked --offline --release`, `CARGO_PROFILE_RELEASE_DEBUG=1`, the same toolchain and private target. The loop includes normal parsed-result destruction. Timing does not include I/O, provider, Git, database or JSON. The input corpus is synthetic although its source bytes are the same file used by the existing real-provider workload.

Nine alternating baseline/candidate pairs, reversing pair order every other run:

| Complete parser | Median per 2,000 parses | Median per parse |
| --- | ---: | ---: |
| Baseline | 1.282299500 s | 641.150 us |
| Line cursor | 0.812329833 s | 406.165 us |

Median reduction: **36.65%**, or **1.579 times** parser throughput. `line-cursor-microbench.json` preserves all 18 samples and exact Debug projection equality. Baseline-only earlier warm samples are separately retained in `baseline-microbench.json`. These are isolated parser results; they are not a full100k search result or provider/production acceptance claim.

## Verification and mutation evidence

- New Given/When/Then test `adjacent_sections_preserve_lines_across_gaps_unicode_and_line_endings` independently asserts literal line coordinates 3–9 and 13–15. It includes CRLF, Unicode prefix/body, an excluded fenced heading, an unmarked terminating heading, an inter-element gap and no final newline.
- `coordinates-baseline.log`: existing behavior passes before candidate implementation.
- `coordinates-utf8-red.log`: first candidate fails on slicing inside final Cyrillic scalar; raw-byte correction is GREEN (`coordinates-green.log`).
- `coordinates-reset-mutation.log`: deliberate loss of cumulative line state after each element fails with second-section start **5 vs expected 13**; restoration passes in `coordinates-restored.log`. The mutation is absent from the retained tree.
- `markdown-suites.log`: **44 tests pass**, covering all five Markdown integration targets (`markdown_elements`, `markdown_search`, `markdown_links`, `markdown_link_paths`, `markdown_link_lifecycle`). Existing golden fixtures independently assert exact byte spans, lines, metadata, hashes and search representations; existing negative tests exercise metadata, nesting, anchor, links and Unicode failures.
- Full `CARGO_TARGET_DIR=../target CARGO_BUILD_JOBS=2 make verify`: **PASS**, exit 0, output `ok`; includes format check, all-target check, Clippy with denied warnings, fixed-value lint, all root debug tests, strict Rustdoc and all root release tests. See `make-verify.log`.

Physical review: exactly `src/markdown/scan.rs` and `tests/markdown_elements.rs` change. All runtime helpers remain private within the same Markdown owner. No visibility, interface, import direction, wiring, storage or native-stack input changes. Storage platform runner is not applicable. Linux/x86 and full100k candidate timing are not performed by this experiment.

## Handoff

`parser-cpu.patch` is the only proposed product/test patch. `candidate-build.json` pins its hash and the copied debug-enabled release executable `letopis-parser-cpu` (SHA-256 `83b990af8b7b16c650fbf3c1f9453d86988e910347e2c665c6210d6dd03947e0`). The private `target` is retained for its debug artifacts. No commits, pushes or root-source edits were made.
