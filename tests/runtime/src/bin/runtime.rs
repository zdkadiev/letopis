use letopis_runtime_checks::{CheckResult, command, run_drain};
use std::{
    env,
    io::{BufRead, BufReader, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::Path,
    time::Duration,
};
fn main() {
    if let Err(e) = run() {
        eprintln!("runtime: {e}");
        std::process::exit(1);
    }
}
fn run() -> CheckResult<()> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|x| x == "--versions") {
        let c = letopis_embedding_checks::open(Path::new(":memory:"))?;
        println!(
            "SQLite {}; vec1 {}",
            rusqlite::version(),
            c.query_row("SELECT vec1_info()", [], |r| r.get::<_, String>(0))?
        );
        return Ok(());
    }
    let root = Path::new(args.get(1).ok_or("root")?);
    let mode = args.get(2).ok_or("mode")?;
    let is_drain = mode == "drain";
    let selected = env::var(if is_drain {
        "TK4_DRAIN_CUTS"
    } else {
        "TK4_COMMAND_CUTS"
    })
    .unwrap_or_default();
    // Fixture-only channel: one owned connection lives until this process exits.
    let mut channel = env::var_os("TK4_CONTROL")
        .map(|p| -> CheckResult<_> {
            let stream = UnixStream::connect(p)?;
            stream.set_read_timeout(Some(Duration::from_secs(20)))?;
            stream.set_write_timeout(Some(Duration::from_secs(20)))?;
            Ok(BufReader::new(stream))
        })
        .transpose()?;
    let mut checkpoint = |name: &str| -> CheckResult<()> {
        if selected.split(',').any(|s| s == name)
            && let Some(channel) = &mut channel
        {
            let null = std::fs::File::open("/dev/null")?;
            let mut streams = Vec::new();
            for fd in 0..3 {
                // SAFETY: fstat writes only these initialized stat objects; all descriptors are inspected, never owned.
                let (mut actual, mut expected): (libc::stat, libc::stat) =
                    unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
                if unsafe { libc::fstat(fd, &mut actual) } != 0
                    || unsafe { libc::fstat(null.as_raw_fd(), &mut expected) } != 0
                {
                    return Err(std::io::Error::last_os_error().into());
                }
                streams.push(serde_json::json!({"mode":actual.st_mode,"rdev":actual.st_rdev,"null_mode":expected.st_mode,"null_rdev":expected.st_rdev}));
            }
            // SAFETY: querying the calling process's session/group has no pointer arguments.
            let event = serde_json::json!({"pid":std::process::id(),"name":name,"sid":unsafe{libc::getsid(0)},"pgid":unsafe{libc::getpgrp()},"streams":streams});
            writeln!(channel.get_mut(), "{event}")?;
            let mut ack = String::new();
            channel.read_line(&mut ack)?;
            if ack == "sigkill\n" {
                // SAFETY: fixture instruction signals the calling child only, never a numeric peer PID.
                unsafe {
                    libc::raise(libc::SIGKILL);
                }
                return Err("self SIGKILL unexpectedly returned".into());
            }
            if ack != "continue\n" {
                return Err("controller closed/aborted".into());
            }
        }
        Ok(())
    };
    checkpoint("ready")?;
    if is_drain {
        run_drain(
            root,
            &mut |model, text| {
                assert_eq!(model, "three");
                Ok(if text == "bravo" {
                    vec![0., 1., 0.]
                } else {
                    vec![1., 0., 0.]
                })
            },
            &mut checkpoint,
        )?;
    } else {
        let executable = env::var_os("TK4_EXECUTABLE")
            .map(std::path::PathBuf::from)
            .unwrap_or(env::current_exe()?);
        let outcome = command(root, mode == "reindex", &executable, &mut checkpoint)?;
        println!("{outcome:?}");
    }
    checkpoint("finished")?;
    Ok(())
}
