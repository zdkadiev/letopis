<!-- letopis: {"created_at":1793278392,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Command wiring

`run_request` selects the typed request, constructs the services required by that command, and returns a typed response. Help can succeed when environment lookup fails; other commands that need the environment use its result. Foreground commands such as check, search and reindex finish through the shared background runner.

[Command dispatch and wiring](src/commands.rs "letopis:type=external")
