use letopis_publication_checks::{CheckResult, publish, reset_config};
use std::{
    env,
    io::{self, Write},
    path::Path,
};

fn main() {
    let result = (|| -> CheckResult<()> {
        let args: Vec<_> = env::args().collect();
        if args.get(1).is_some_and(|arg| arg == "--versions") {
            let conn = letopis_publication_checks::open(Path::new(":memory:"))?;
            let vec1: String = conn.query_row("SELECT vec1_info()", [], |r| r.get(0))?;
            println!("SQLite {}; vec1 {vec1}", rusqlite::version());
            return Ok(());
        }
        let root = Path::new(args.get(1).ok_or("missing root")?);
        let mode = args.get(2).ok_or("missing mode")?;
        let selected = args.get(3).map(String::as_str).unwrap_or("");
        let mut checkpoint = |name: &str| -> CheckResult<()> {
            if selected.split(',').any(|x| x == name) {
                println!("checkpoint:{name}");
                io::stdout().flush()?;
                let mut line = String::new();
                if io::stdin().read_line(&mut line)? == 0 {
                    return Err("checkpoint control pipe closed".into());
                }
            }
            Ok(())
        };
        match mode.as_str() {
            "publish" | "reindex" => publish(root, mode == "reindex", &mut checkpoint)?,
            "reset" => reset_config(root, &mut checkpoint)?,
            _ => return Err("unknown mode".into()),
        }
        println!("done");
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("publisher: {e}");
        std::process::exit(1)
    }
}
