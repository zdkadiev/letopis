use std::{env, io, process::ExitCode};

fn main() -> ExitCode {
    ExitCode::from(letopis::run_cli(
        env::args_os().skip(1),
        io::stdin().lock(),
        io::stdout().lock(),
    ))
}
