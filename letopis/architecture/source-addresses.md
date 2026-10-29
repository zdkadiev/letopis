<!-- letopis: {"created_at":1793278392,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Source addresses

`SourceUri` distinguishes normalized repository-relative paths, optionally with inclusive line ranges, from opaque absolute HTTP/HTTPS addresses. Remote addresses retain their original spelling and are not fetched by parsing. Local range selection returns exact bytes including line endings and fails when the full range is unavailable.

[Source address implementation](src/domain/source.rs "letopis:type=external")
