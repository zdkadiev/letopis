"""Read-only physical breakdown. Python SQLite version is recorded separately."""
import json
from pathlib import Path
import sqlite3
import sys

root = Path(__file__).resolve().parent
database = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "capacity.db"
connection = sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True)
result = {"observer_sqlite": sqlite3.sqlite_version}
for name in ["page_size", "page_count", "freelist_count"]:
    result[name] = connection.execute("PRAGMA " + name).fetchone()[0]
connection.row_factory = sqlite3.Row
result["dbstat"] = [dict(row) for row in connection.execute(
    "SELECT name,count(*) AS pages,sum(pgsize) AS bytes,sum(payload) AS payload_bytes "
    "FROM dbstat GROUP BY name ORDER BY name")]
connection.row_factory = None
result["node_meta"] = connection.execute(
    "SELECT kind,count(*),min(length(uri)),max(length(uri)),min(length(hash)),max(length(hash)) "
    "FROM nodes GROUP BY kind ORDER BY kind").fetchall()
result["edge_types"] = connection.execute(
    "SELECT type,count(*) FROM edges GROUP BY type ORDER BY type").fetchall()
result["additional_checks"] = {
    "uuid_disjoint_nodes_edges": connection.execute(
        "SELECT NOT EXISTS(SELECT 1 FROM nodes n JOIN edges e ON n.id=e.id)").fetchone()[0] == 1,
    "includes_sources_metavertices": connection.execute(
        "SELECT NOT EXISTS(SELECT 1 FROM edges e JOIN nodes n ON n.id=e.source_id "
        "WHERE e.type='includes' AND n.kind<>'metavertex')").fetchone()[0] == 1,
    "includes_no_metavertex_targets_in_fixture": connection.execute(
        "SELECT NOT EXISTS(SELECT 1 FROM edges e JOIN nodes n ON n.id=e.target_id "
        "WHERE e.type='includes' AND n.kind='metavertex')").fetchone()[0] == 1,
}
connection.close()
print(json.dumps(result, indent=2))
