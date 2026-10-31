<!-- letopis: {"created_at":1793278527,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Markdown projection and publication

Marked Markdown is authored knowledge; the document refresh service discovers candidate files, captures and parses them, and publishes a derived graph and search projection through storage. The refresh service owns the index lock and rechecks changes under it before publication. Routine refresh retains source roots; verified refresh and reindex perform closure verification. Reindex requests a full projection while ordinary refresh is incremental.

[Source: src/service/document_refresher.rs](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

[Source: src/markdown/mod.rs](../../src/markdown/mod.rs "letopis:type=external;path=src/markdown/mod.rs")
