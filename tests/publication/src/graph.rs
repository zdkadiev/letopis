use crate::CheckResult;
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Edge = (String, String, String);

fn visible(conn: &Connection) -> CheckResult<Vec<Edge>> {
    Ok(conn.prepare("SELECT source_id,target_id,type FROM edges JOIN nodes ON nodes.id=edges.target_id ORDER BY source_id,target_id,type")?
        .query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?)
}

pub fn traverse(conn: &Connection, start: &str) -> CheckResult<Vec<Edge>> {
    let edges = visible(conn)?;
    let mut seen = BTreeSet::from([start.to_owned()]);
    let mut queue = VecDeque::from([start.to_owned()]);
    let mut out = Vec::new();
    while let Some(node) = queue.pop_front() {
        for edge in edges.iter().filter(|e| e.0 == node) {
            out.push(edge.clone());
            if seen.insert(edge.1.clone()) {
                queue.push_back(edge.1.clone())
            }
        }
    }
    Ok(out)
}

pub fn diagnostics(conn: &Connection) -> CheckResult<Vec<String>> {
    let mut out: BTreeSet<String> = conn
        .prepare("SELECT path FROM files WHERE error IS NOT NULL")?
        .query_map([], |r| Ok(format!("file:{}", r.get::<_, String>(0)?)))?
        .collect::<rusqlite::Result<_>>()?;
    out.extend(conn.prepare("SELECT source_id,target_id FROM edges LEFT JOIN nodes ON nodes.id=edges.target_id WHERE nodes.id IS NULL")?.query_map([],|r|Ok(format!("dangling:{}->{}",r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?);
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (source, target, kind) in visible(conn)? {
        if kind == "includes" {
            graph.entry(source).or_default().push(target);
        }
    }
    fn cycles(
        start: &str,
        path: &mut Vec<String>,
        graph: &BTreeMap<String, Vec<String>>,
        out: &mut BTreeSet<String>,
    ) {
        if let Some(targets) = graph.get(path.last().unwrap()) {
            for t in targets {
                if t == start {
                    out.insert(format!("cycle:{}->{start}", path.join("->")));
                } else if t.as_str() > start && !path.contains(t) {
                    path.push(t.clone());
                    cycles(start, path, graph, out);
                    path.pop();
                }
            }
        }
    }
    for start in graph.keys() {
        cycles(start, &mut vec![start.clone()], &graph, &mut out)
    }
    Ok(out.into_iter().collect())
}
