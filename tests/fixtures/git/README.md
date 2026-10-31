# SHA-mbles collision fixture

`sha-mbles-1.bin` is the 640-byte unprefixed test vector copied verbatim from
`sha1-checked` 0.10.0, `tests/data/sha-mbles-1.bin` (MIT OR Apache-2.0),
https://crates.io/crates/sha1-checked/0.10.0.
SHA-256: `3ead211681cec93d265c8ac123dd062e105408cebf82fa6e2b126f4f40bcb88c`.
The unit test checks collision-error propagation in the existing hasher; it does
not claim a Git-header-prefixed version of this published attack still collides.
