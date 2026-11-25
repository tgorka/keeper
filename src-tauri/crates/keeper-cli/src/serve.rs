//! `keeper-cli serve --state <dir>` (story 123.3, DR-5): the wire switched on
//! at a terminal, on a keeper-cli device's own state. It claims the state
//! (the marker), takes the state's lock (a second `serve` exits `3`), opens
//! one replica per `--database` as the verbs do and keeps them linked, and
//! serves `<state>/wire/wire.sock` until SIGINT or SIGTERM. Until 123.4 only
//! the `app` reads and `describe` answer; `--domain` and `--writes` are
//! 123.4's.
//!
//! Exit codes: `0` stopped by a signal; `3` the state is not keeper-cli's,
//! is unsafe, is locked by another process, cannot be locked here, or its
//! socket's place is refused; `1` anything else.

use std::path::PathBuf;
use std::sync::Arc;

use keeper_wire::{Hooks, Limits, Server};
use keeper_wire_api::{DataLock, LockError};
use serde_json::json;

use crate::output::Printer;
use crate::{EXIT_FAILURE, EXIT_OK, EXIT_PREREQUISITE};

/// `keeper-cli serve`'s arguments.
#[derive(clap::Args)]
pub struct Args {
    /// The keeper-cli state directory to serve.
    #[arg(long)]
    state: PathBuf,
    /// keeper-hq's base URL (`<url>/<database>` is the mount).
    #[arg(long)]
    url: String,
    /// A database to open and keep linked; repeat it for more.
    #[arg(long, required = true)]
    database: Vec<String>,
    /// The person's actor id: once, or once per `--database`.
    #[arg(long, required = true)]
    actor: Vec<String>,
    /// This device's client id: once, or once per `--database`.
    #[arg(long, required = true)]
    client: Vec<String>,
}

fn refuse(printer: &Printer, code: &str, detail: &str) -> u8 {
    let answer = json!({"ok": false, "error": code, "detail": detail});
    printer.answer(&answer, |_| detail.to_owned());
    EXIT_PREREQUISITE
}

/// Serve until a signal, and return the exit code.
pub fn main(args: Args, printer: &Printer) -> u8 {
    if let Err(sentence) = crate::state::claim(&args.state) {
        return refuse(printer, "state_not_ours", &sentence);
    }
    let holder = format!("keeper-cli serve, pid {}", std::process::id());
    let lock = match DataLock::acquire(&args.state, &holder) {
        Ok(lock) => Arc::new(lock),
        Err(
            error @ (LockError::Held { .. } | LockError::Unsafe(_) | LockError::Unsupported { .. }),
        ) => {
            return refuse(printer, error.code(), &error.to_string());
        }
        Err(error) => {
            let answer = json!({"ok": false, "error": error.code(), "detail": error.to_string()});
            printer.answer(&answer, |_| error.to_string());
            return EXIT_FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let answer = json!({"ok": false, "error": "runtime", "detail": error.to_string()});
            printer.answer(&answer, |_| error.to_string());
            return EXIT_FAILURE;
        }
    };
    runtime.block_on(serve(args, lock, printer))
}

async fn serve(args: Args, lock: Arc<DataLock>, printer: &Printer) -> u8 {
    let (_, bindings) = match crate::graph::open_domains(
        &args.state,
        &args.url,
        &args.database,
        &args.actor,
        &args.client,
    ) {
        Ok(opened) => opened,
        Err(answer) => {
            printer.answer(&answer, |a| {
                a["detail"].as_str().unwrap_or_default().to_owned()
            });
            return crate::EXIT_CONFIG;
        }
    };
    let config = crate::graph::serve_config(&args.state);
    let runtime = match crate::compose::build(config, bindings) {
        Ok((runtime, _)) => runtime,
        Err(error) => {
            let answer =
                json!({"ok": false, "error": "start_refused", "detail": error.to_string()});
            printer.answer(&answer, |_| error.to_string());
            return EXIT_PREREQUISITE;
        }
    };
    let dispatch = Arc::new(runtime.wire_dispatcher());
    let server = match Server::start(lock, dispatch, Limits::default(), Hooks::default()) {
        Ok(server) => server,
        Err(error) => return refuse(printer, error.code(), &error.to_string()),
    };
    let socket = server.socket().display().to_string();
    printer.answer(&json!({"ok": true, "serving": socket}), |_| {
        format!("keeper-cli is serving {socket}; stop it with Ctrl-C")
    });
    wait_for_signal().await;
    server.stop().await;
    EXIT_OK
}

async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut interrupt), Ok(mut terminate)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
    ) else {
        // No signal handler: serve until killed.
        std::future::pending::<()>().await;
        return;
    };
    tokio::select! {
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
    }
}
