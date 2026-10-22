# TK-8 evidence

Implementation report: [report.md](report.md). Independent acceptance is controller-owned.

`verification.json` records actual command argv/status, Git/Rust/platform/image metadata,
SHA-256 for source/test/config inputs and the full evidence logs. `tdd/` keeps compiling
behavioral RED/GREEN and the two self-review regression pairs. `logs/` holds final host,
network-disabled Linux and inherited-routing verification. Existing TK-7 receipts are unchanged.

Reproduce using `tests/git/README.md`. Physical durability, actual Git 2.29 execution,
x86_64, atomic live capture and resource limits are not proven here.
