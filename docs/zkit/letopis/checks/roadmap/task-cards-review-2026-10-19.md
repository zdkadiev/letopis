# Task cards review — 2026-10-19

Scope: 45 Markdown task descriptions and a proposed standard; no product implementation, measurements, provider calls or drills.

## Request and result

D-87 separates the user's explicit placement/content request from the author's proposed format. The tasks directory contains exactly 45 files, TK-01.md…TK-45.md, one card for each original task ID. `task-template.md` is outside the directory. Roadmap remains the scheduling registry and now links to cards for detailed work and acceptance.

Each card contains status, phase, effort range, weekly hours, dependency links; goal, context/inputs, specific actions, expected deliverable, acceptance checklist, verification method, constraints and future actual report. All cards remain planned and all acceptance boxes unchecked. Final TK-45 hands the user an accepted v1 build/report; it does not refer to a nonexistent next task or declare readiness now.

## Independent review

Fresh read-only reviewer `/root/task_cards_review` read the cards/standard and compared the earlier roadmap. Verdict: **consistent**, no material findings.

Reviewer confirmed preservation of all 45 tasks, criteria, names, IDs, phases, effort ranges, dependencies, weekly plan and scope. Previous-phase gates are prerequisites; the current gate closes at the phase's last task. Links point to existing documents and decision anchors. No new product requirements were introduced through template boilerplate. The proposed standard is not represented as a separately approved user schema.

## Mechanical evidence

`task-cards-checks.json` records:

- weekly calculator/card checks and standard zkit roadmap check exit 0;
- 45 cards and 45 tasks, expected 372 hours / 38 weeks / max 10 hours per week / last week 2 hours, terminal TK-45;
- original acceptance preserved word-for-word after normalizing punctuation/case/checklist markers;
- 640 local file links verified;
- isolated negative copies with a missing card, stale estimate and missing acceptance section all rejected;
- original resource model, resource-math and measurement artifacts unchanged.

`roadmap_schedule.py --check` detects card title/status/phase/effort/weekly/dependency drift and missing mandatory sections. It validates documentation structure, not runtime success or the truth of future completion claims. The task report must contain actual evidence before done.

The existing plan's inherited draft status and open resource/contracts gates are unaffected. Format and file organization are complete; task execution has not started.
