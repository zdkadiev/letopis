// Additional read-only concurrency measurement; not a product worker pool.
use rusqlite::{Connection, LoadExtensionGuard, OpenFlags, params};
use std::{
    env,
    error::Error,
    io::{self, Write},
    thread,
    time::{Duration, Instant},
};
type R<T> = Result<T, Box<dyn Error + Send + Sync>>;
fn open(path: &str, ext: &str) -> R<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    unsafe {
        let _g = LoadExtensionGuard::new(&c)?;
        c.load_extension(ext, Some("sqlite3_extension_init"))?;
    }
    c.pragma_update(None, "synchronous", "FULL")?;
    c.pragma_update(None, "fullfsync", true)?;
    c.pragma_update(None, "foreign_keys", true)?;
    Ok(c)
}
fn phase(name: &str) {
    println!(
        "{{\"event\":\"phase\",\"phase\":\"{name}\",\"pid\":{}}}",
        std::process::id()
    );
    io::stdout().flush().unwrap();
    thread::sleep(Duration::from_millis(300));
}
fn query(c: &mut Connection, known: i64) -> R<usize> {
    let tx = c.transaction()?;
    let ft = {
        let mut s=tx.prepare("SELECT rowid FROM text_index WHERE text_index MATCH 'graph AND memory' ORDER BY rank LIMIT 10")?;
        s.query_map([], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    assert_eq!(ft.len(), 10);
    let vector: Vec<u8> = tx.query_row(
        "SELECT vector FROM vector_index WHERE rowid=?1",
        [known],
        |r| r.get(0),
    )?;
    let a = {
        let mut s = tx.prepare("SELECT rowid FROM vector_index(?1,'{\"K\":10}')")?;
        s.query_map(params![vector], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    assert_eq!(a.len(), 10);
    assert_eq!(a[0], known);
    tx.commit()?;
    Ok(ft.len() + a.len())
}
fn main() -> R<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 || args[1] != "mixed" {
        return Err("usage: mixed mixed DB EXTENSION".into());
    }
    let path = &args[2];
    let ext = &args[3];
    let begin = Instant::now();
    phase("mixed_open");
    let start = Instant::now() + Duration::from_millis(500);
    let jobs:Vec<_>=(0..10).map(|worker|{let path=path.clone();let ext=ext.clone();thread::spawn(move ||->R<()>{let mut c=open(&path,&ext)?;thread::sleep(start.saturating_duration_since(Instant::now()));let t=Instant::now();let count=query(&mut c,1+(worker*1901)%100000)?;println!("{{\"event\":\"query\",\"kind\":\"burst_10_readers\",\"worker\":{worker},\"ms\":{},\"rows\":{count}}}",t.elapsed().as_secs_f64()*1000.);Ok(())})}).collect();
    for job in jobs {
        job.join().expect("reader panic")?;
    }
    phase("burst_done");
    let start = Instant::now() + Duration::from_millis(500);
    let jobs:Vec<_>=(0..10).map(|worker|{let path=path.clone();let ext=ext.clone();thread::spawn(move ||->R<()>{let mut c=open(&path,&ext)?;for round in 0..15 {let i=round*10+worker;let scheduled=start+Duration::from_millis((i*100) as u64);thread::sleep(scheduled.saturating_duration_since(Instant::now()));let t=Instant::now();let lag_ms=t.saturating_duration_since(scheduled).as_secs_f64()*1000.;let count=query(&mut c,1+((i*1901)%100000) as i64)?;println!("{{\"event\":\"query\",\"kind\":\"steady_10qps\",\"i\":{i},\"worker\":{worker},\"dispatch_lag_ms\":{lag_ms},\"ms\":{},\"rows\":{count}}}",t.elapsed().as_secs_f64()*1000.);}Ok(())})}).collect();
    for job in jobs {
        job.join().expect("reader panic")?;
    }
    phase("steady_done");
    let c = open(path, ext)?;
    let integrity: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    assert_eq!(integrity, "ok");
    println!("{{\"event\":\"verify\",\"result\":\"PASS\"}}");
    println!(
        "{{\"event\":\"complete\",\"mode\":\"mixed\",\"seconds\":{}}}",
        begin.elapsed().as_secs_f64()
    );
    Ok(())
}
