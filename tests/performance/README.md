# Model-free performance diagnosis (#65)

Use `/opt/homebrew/bin/python3.12 -B` on this host; system Python is too old.
The runner uses new synthetic standalone repositories, explicit disabled/fake
embedding settings and immutable evidence. It never uses the working repository's
`.letopis` configuration or calls a real provider.

`python3 -B tests/performance/run.py --list-scenarios` lists scenarios without
building or running the product. Execution stages are added by the approved plan.

Build inputs, release executable and harness are pinned separately. Cargo builds,
fixture generation, projection initialization and independent sample resets are
setup, outside end-to-end CLI latency. Each measured call starts a new process.
Copying changes file inodes, so copied projections are refreshed before edits.

Raw output belongs in a new ignored `.artifacts/issue-65/` directory. Never replace
an earlier run or label uncontrolled page-cache state cold. These synthetic probes
and fake-provider overhead do not establish true hybrid latency, model cost,
real production relevance, or any formal project gate.
