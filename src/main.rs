use std::{env, io, process::ExitCode};

fn main() -> ExitCode {
    ExitCode::from(letopis::run_cli(
        env::args_os().skip(1),
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
    ))
}
