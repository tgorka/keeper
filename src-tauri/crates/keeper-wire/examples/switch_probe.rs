//! Probe binary built twice (debug and release) by AC13's test (story 123.3).
//!
//! Usage: `switch_probe <data_dir> <for_ms>`.
//! Acquires the data-directory lock, builds a minimal runtime with only
//! `app_module()`, and runs [`keeper_wire::follow_switch`] gated solely by
//! [`keeper_wire::debug_override`] for `<for_ms>` milliseconds.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use keeper_runtime::{app_module, AppInfo, Runtime};
use keeper_wire::{debug_override, follow_switch, Dispatch, Limits};
use keeper_wire_api::DataLock;

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let data_dir = PathBuf::from(
        args.next()
            .ok_or_else(|| "usage: switch_probe <data_dir> <for_ms>".to_owned())?,
    );
    let for_ms: u64 = args
        .next()
        .ok_or_else(|| "usage: switch_probe <data_dir> <for_ms>".to_owned())?
        .to_str()
        .ok_or_else(|| "for_ms must be valid UTF-8".to_owned())?
        .parse()
        .map_err(|err| format!("invalid for_ms: {err}"))?;

    let lock = Arc::new(
        DataLock::acquire(&data_dir, "switch_probe")
            .map_err(|err| format!("acquire data lock: {err}"))?,
    );

    let info = AppInfo {
        version: "0.0.0-probe".to_owned(),
        build: "switch-probe".to_owned(),
        snapshot_digest: "probe".to_owned(),
        domains: Vec::new(),
    };
    let (runtime, _local) =
        Runtime::build(info, vec![app_module()]).map_err(|err| format!("build runtime: {err}"))?;
    let dispatcher: Arc<dyn Dispatch> = Arc::new(runtime.wire_dispatcher());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("build tokio runtime: {err}"))?;

    rt.block_on(async move {
        let (quit_tx, quit_rx) = tokio::sync::watch::channel(false);
        let stopper = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(for_ms)).await;
            let _ = quit_tx.send(true);
        });
        follow_switch(
            lock,
            debug_override,
            move || Ok(Arc::clone(&dispatcher)),
            Limits::default(),
            quit_rx,
        )
        .await;
        let _ = stopper.await;
    });

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("switch_probe: {err}");
            ExitCode::FAILURE
        }
    }
}
