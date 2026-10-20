use letopis_storage_build::{CheckResult, open_checked};
use std::{env, path::Path};

fn main() {
    if let Err(error) = check() {
        eprintln!("unsupported storage build: {error}");
        std::process::exit(1);
    }
}

fn check() -> CheckResult<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: check NEW_DB VEC1_EXTENSION".into());
    }
    let path = Path::new(&args[0]);
    if path.exists() {
        return Err("NEW_DB must not already exist".into());
    }
    // SAFETY: this example is invoked by run.py with its verified vec1 build (or
    // a missing/plain-text negative fixture). Do not pass an untrusted native library.
    let conn = unsafe { open_checked(path, Path::new(&args[1]))? };
    let vec1: String = conn.query_row("SELECT vec1_info()", [], |row| row.get(0))?;
    println!("SQLite {} / {vec1}: supported", rusqlite::version());
    Ok(())
}
