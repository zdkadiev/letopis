use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(letopis::run_process_cli())
}
