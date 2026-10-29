<!-- letopis: {"created_at":1793278527,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Search pipeline

Search refreshes the document projection, validates source-revision filters, reads embedding configuration, and opens a storage read view. It obtains text and available vector candidates, checks candidate eligibility and source provenance, then ranks the eligible results. Results are metadata links rather than the knowledge text; source text can be read from an inspect snapshot.

[Source: src/service/search.rs](src/service/search.rs "letopis:type=external")

[Source: src/cli/help.rs](src/cli/help.rs "letopis:type=external")
