//! Minimal `keeper.wire.v1` probe: `wire-probe [--data-dir DIR] <operation|describe> [json-input]`.

use std::path::PathBuf;
use std::process::ExitCode;

use keeper_wire_api::client::{CallOptions, Client};
use keeper_wire_api::{Describe, WireError};
use serde::Serialize;
use serde_json::{json, Value};

const USAGE: &str = "usage: wire-probe [--data-dir DIR] <operation|describe> [json-input]";
const CLIENT_NAME: &str = concat!("wire-probe/", env!("CARGO_PKG_VERSION"));

struct Args {
    data_dir: PathBuf,
    operation: String,
    input: Value,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut data_dir: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();

    while let Some(arg) = args.next() {
        if arg == "--data-dir" {
            let dir = args
                .next()
                .ok_or_else(|| format!("--data-dir requires a path\n{USAGE}"))?;
            data_dir = Some(PathBuf::from(dir));
        } else if let Some(dir) = arg.strip_prefix("--data-dir=") {
            if dir.is_empty() {
                return Err(format!("--data-dir requires a path\n{USAGE}"));
            }
            data_dir = Some(PathBuf::from(dir));
        } else {
            positional.push(arg);
        }
    }

    if positional.is_empty() || positional.len() > 2 {
        return Err(USAGE.to_owned());
    }

    let data_dir = match data_dir {
        Some(dir) => dir,
        None => std::env::var_os("KEEPER_WIRE_DATA_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| format!("set --data-dir or KEEPER_WIRE_DATA_DIR\n{USAGE}"))?,
    };

    let mut iter = positional.into_iter();
    let operation = iter.next().ok_or_else(|| USAGE.to_owned())?;
    let input = match iter.next() {
        Some(raw) => serde_json::from_str::<Value>(&raw)
            .map_err(|err| format!("invalid json-input: {err}"))?,
        None => json!({}),
    };

    Ok(Args {
        data_dir,
        operation,
        input,
    })
}

fn print_output<T: Serialize>(value: &T) -> ExitCode {
    match serde_json::to_string(value) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn print_wire_error(err: &WireError) -> ExitCode {
    match serde_json::to_string(err) {
        Ok(line) => eprintln!("{line}"),
        Err(ser) => eprintln!("{ser}"),
    }
    ExitCode::FAILURE
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };

    let client = match Client::connect(&args.data_dir, CLIENT_NAME).await {
        Ok(client) => client,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    if args.operation == "describe" {
        match client.describe(Describe::default()).await {
            Ok(output) => print_output(&output),
            Err(err) => print_wire_error(&err),
        }
    } else {
        match client
            .call(&args.operation, args.input, CallOptions::default())
            .await
        {
            Ok(output) => print_output(&output),
            Err(err) => print_wire_error(&err),
        }
    }
}
