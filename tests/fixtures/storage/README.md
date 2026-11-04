# Frozen storage schemas

`cache-v3.sql` is the byte-exact cache schema captured from
`src/storage/cache.sql` at revision `166aaa8c059aabe38814f689d739dcd0833d7a7a`
before the schema-4 integer-edge change. Its SHA-256 is
`ddabeba5678cd62cbb68367f595a71a3864ebf0a55c255a5630563d1c49f400a`.
It has integer node IDs and textual edge endpoints; consumers set
`pragma user_version=3` separately, as the captured DDL does not set a version.

The existing schema-1/2 fixtures are historical compatibility evidence and remain
unchanged. New layouts belong in separate fixtures.
