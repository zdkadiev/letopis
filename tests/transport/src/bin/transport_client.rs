//! One-shot proof CLI. Fixture flags are not product command/configuration APIs.
use letopis_transport_checks::{
    client::Client,
    protocol::{Config, TransportError},
};
use serde::Deserialize;
use serde_json::{Value, json};
use signal_hook::{consts::SIGINT, iterator::Signals};
use std::{
    io::{self, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct Output(Mutex<bool>);
impl Output {
    fn write_once(&self, value: &Value) -> bool {
        let mut printed = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if !*printed {
            *printed = true;
            let mut bytes = serde_json::to_vec(value).expect("JSON value is serializable");
            bytes.push(b'\n');
            let mut stdout = io::stdout().lock();
            if stdout
                .write_all(&bytes)
                .and_then(|_| stdout.flush())
                .is_err()
            {
                std::process::exit(1);
            }
            true
        } else {
            false
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    config: Value,
    texts: Vec<String>,
    expected_dimensions: usize,
}

fn error_value(error: TransportError) -> Value {
    json!({"ok":false,"error":{"code":if error == TransportError::Cancelled {"cancelled"} else {"embedding_failed"},"message":error.to_string(),"details":{}}})
}

fn run(args: &[String]) -> Result<Value, TransportError> {
    let mut deadline = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--deadline-ms" => {
                i += 1;
                deadline = Some(Duration::from_millis(
                    args.get(i)
                        .and_then(|v| v.parse().ok())
                        .ok_or(TransportError::Config)?,
                ));
            }
            "--before-output-gate" => {
                i += 1;
                args.get(i).ok_or(TransportError::Config)?;
            }
            "--hold-after-output"
            | "--hold-after-cancel-output"
            | "--drain-fixture"
            | "--tls-probe" => {}
            _ => return Err(TransportError::Config),
        }
        i += 1;
    }
    let client = match deadline {
        Some(d) => Client::with_fixture_policy(d)?,
        None => Client::new()?,
    };
    if args.iter().any(|a| a == "--tls-probe") {
        client.system_tls_probe()?;
        return Ok(
            json!({"ok":true,"result":{"system_tls":"verified","endpoint":"https://example.com/","inference":false}}),
        );
    }
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|_| TransportError::Config)?;
    let input: Input = serde_json::from_slice(&bytes).map_err(|_| TransportError::Config)?;
    let config =
        Config::parse(&serde_json::to_vec(&input.config).map_err(|_| TransportError::Config)?)?;
    let vectors = client.embed(&config, &input.texts, input.expected_dimensions)?;
    Ok(json!({"ok":true,"result":vectors}))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = Arc::new(Output::default());
    if !args.iter().any(|a| a == "--drain-fixture") {
        let mut signals = match Signals::new([SIGINT]) {
            Ok(signals) => signals,
            Err(_) => {
                output.write_once(&error_value(TransportError::Connection));
                std::process::exit(1);
            }
        };
        let signal_output = Arc::clone(&output);
        let hold_after_cancel = args.iter().any(|a| a == "--hold-after-cancel-output");
        if std::thread::Builder::new()
            .name("sigint".into())
            .spawn(move || {
                if signals.forever().next().is_some() {
                    signal_output.write_once(&error_value(TransportError::Cancelled));
                    if hold_after_cancel {
                        loop {
                            std::thread::park();
                        }
                    }
                    std::process::exit(1);
                }
            })
            .is_err()
        {
            output.write_once(&error_value(TransportError::Connection));
            std::process::exit(1);
        }
    }
    let result = run(&args);
    let (value, code) = match result {
        Ok(value) => (value, 0),
        Err(error) => (error_value(error), 1),
    };
    if let Some(position) = args.iter().position(|a| a == "--before-output-gate")
        && let Some(path) = args.get(position + 1)
    {
        let gate = std::path::Path::new(path);
        if std::fs::write(
            gate.with_extension("ready"),
            if code == 0 { "success" } else { "failure" },
        )
        .is_err()
        {
            output.write_once(&error_value(TransportError::Config));
            std::process::exit(1);
        }
        while !gate.exists() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let won_output = output.write_once(&value);
    if args.iter().any(|a| a == "--hold-after-output") {
        loop {
            std::thread::park();
        }
    }
    // If cancellation printed first, main cannot turn it into exit 0.
    std::process::exit(if won_output { code } else { 1 });
}
