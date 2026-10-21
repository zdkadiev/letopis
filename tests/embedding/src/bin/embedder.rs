use letopis_embedding_checks::{CheckResult, capture_config, drain, open};
use std::{
    env,
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
};
fn main() {
    if let Err(e) = run() {
        eprintln!("embedder: {e}");
        std::process::exit(1)
    }
}
fn run() -> CheckResult<()> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|x| x == "--versions") {
        let c = open(Path::new(":memory:"))?;
        println!(
            "SQLite {}; vec1 {}",
            rusqlite::version(),
            c.query_row("SELECT vec1_info()", [], |r| r.get::<_, String>(0))?
        );
        return Ok(());
    }
    let root = Path::new(args.get(1).ok_or("root")?);
    let mode = args.get(2).ok_or("mode")?;
    let selected = args.get(3).map(String::as_str).unwrap_or("");
    let cfg = capture_config(root)?;
    let mut provider = |model: &str, text: &str| -> CheckResult<Vec<f32>> {
        assert_eq!(model, cfg.model);
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(".letopis/requests.jsonl"))?;
        writeln!(log, "{}", serde_json::json!({"model":model,"text":text}))?;
        log.sync_all()?;
        match mode.as_str() {
            "error" => Err("fixture provider error".into()),
            "empty" => Ok(vec![]),
            "nan" => Ok(vec![f32::NAN, 0., 0.]),
            "wrong" => Ok(vec![1., 0.]),
            "valid" => {
                let mut v = vec![0.; if model == "three" { 3 } else { 4 }];
                v[usize::from(text == "bravo")] = 1.;
                Ok(v)
            }
            _ => Err("mode".into()),
        }
    };
    let mut checkpoint = |name: &str| -> CheckResult<()> {
        if selected.split(',').any(|s| s == name) {
            println!("checkpoint:{name}");
            io::stdout().flush()?;
            let mut line = String::new();
            if io::stdin().read_line(&mut line)? == 0 {
                return Err("control pipe closed".into());
            }
        }
        Ok(())
    };
    drain(root, &mut provider, &mut checkpoint)?;
    println!("done");
    Ok(())
}
