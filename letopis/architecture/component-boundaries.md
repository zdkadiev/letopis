<!-- letopis: {"created_at":1793278527,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Component boundaries

Letopis uses a hexagonal structure: domain types and pure rules live in `src/domain`, application-owned interfaces and services live in `src/service`, and technology implementations live in dedicated modules such as `git`, `storage`, `filesystem`, `markdown`, and `embedding`. The CLI parses and formats requests; `commands.rs` dispatches them. `src/lib.rs` composes concrete collaborators into application services. The architecture document states the intended dependency direction and responsibilities; the linked modules show the current composition.

[Source: docs/zkit/letopis/architecture.md](docs/zkit/letopis/architecture.md "letopis:type=external")

[Source: src/lib.rs](src/lib.rs "letopis:type=external")

[Source: src/service/mod.rs](src/service/mod.rs "letopis:type=external")
