//! Test composition of the macOS app's start (story 123.3, SR-14): the same
//! order as `crates/keeper/src/wire.rs` — the lock before any data, then the
//! settings table and the replicas, then the switch.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_graph::module::DomainBinding;
use keeper_ports::{ReplicaSpec, Source};
use keeper_replica::SyncularReplica;
use keeper_wire::{Dispatch, Limits};
use keeper_wire_api::{DataLock, LockError};
use serde_json::{json, Value};

use crate::output::Printer;
use crate::{EXIT_FAILURE, EXIT_OK, EXIT_PREREQUISITE};

/// `keeper-cli debug app-start`'s arguments.
#[derive(clap::Args)]
pub struct Args {
    /// Data directory to lock, open, and serve from.
    #[arg(long)]
    data: PathBuf,
    /// Database to open; repeat for more.
    #[arg(long, required = true)]
    database: Vec<String>,
    /// Actor id shared across opened databases.
    #[arg(long, default_value = "a1")]
    actor: String,
    /// Client id shared across opened databases.
    #[arg(long, default_value = "c1")]
    client: String,
    /// How long to wait for the data directory lock, in milliseconds.
    #[arg(long, default_value_t = 10_000)]
    wait_ms: u64,
    /// Stop automatically after this many milliseconds.
    #[arg(long)]
    for_ms: Option<u64>,
}

struct Settings {
    conn: Mutex<rusqlite::Connection>,
}

impl Settings {
    fn open(data_dir: &Path) -> Result<Self, rusqlite::Error> {
        let conn = rusqlite::Connection::open(data_dir.join("keeper.db"))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare_cached("SELECT value FROM settings WHERE key = ?1")
            .ok()?;
        stmt.query_row([key], |row| row.get::<_, String>(0)).ok()
    }
}

fn emit(printer: &Printer, value: &Value) {
    printer.answer(value, ToString::to_string);
}

/// Run the app-start composition and return the exit code.
pub fn main(args: Args, printer: &Printer) -> u8 {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .try_init();

    // 1. Lock before opening any data.
    let holder = format!("keeper-cli debug app-start, pid {}", std::process::id());
    let lock =
        match DataLock::acquire_waiting(&args.data, &holder, Duration::from_millis(args.wait_ms)) {
            Ok(lock) => Some(Arc::new(lock)),
            Err(error @ (LockError::Held { .. } | LockError::Unsafe(_))) => {
                emit(
                    printer,
                    &json!({"ok": false, "error": error.code(), "detail": error.to_string()}),
                );
                return EXIT_PREREQUISITE;
            }
            Err(error @ LockError::Unsupported { .. }) => {
                tracing::warn!(
                    code = error.code(),
                    %error,
                    "data directory lock is unsupported; wire stays off"
                );
                None
            }
            Err(error @ LockError::Io { .. }) => {
                emit(
                    printer,
                    &json!({"ok": false, "error": error.code(), "detail": error.to_string()}),
                );
                return EXIT_FAILURE;
            }
        };

    // 2. Settings table.
    let settings = match Settings::open(&args.data) {
        Ok(settings) => Arc::new(settings),
        Err(error) => {
            emit(
                printer,
                &json!({"ok": false, "error": "settings_open", "detail": error.to_string()}),
            );
            return EXIT_FAILURE;
        }
    };

    // 3. Replicas.
    let hq_dir = args.data.join("hq");
    if let Err(error) = std::fs::create_dir_all(&hq_dir) {
        emit(
            printer,
            &json!({"ok": false, "error": "storage_unavailable", "detail": error.to_string()}),
        );
        return EXIT_FAILURE;
    }
    let mut sources = Vec::with_capacity(args.database.len());
    for database in &args.database {
        let replica = match SyncularReplica::open(ReplicaSpec {
            database: database.clone(),
            path: hq_dir.join(format!("{database}.sqlite")),
            client_id: Some(args.client.clone()),
            link: None,
            units: Vec::new(),
            watch_presence: None,
        }) {
            Ok(replica) => replica,
            Err(error) => {
                emit(
                    printer,
                    &json!({"ok": false, "error": "storage_unavailable", "detail": error.to_string()}),
                );
                return EXIT_FAILURE;
            }
        };
        sources.push(Source {
            database: database.clone(),
            replica: Arc::new(replica),
            actor: args.actor.clone(),
            client: args.client.clone(),
        });
    }
    let sources = Arc::new(sources);

    // 4. Started line.
    let lock_state = if lock.is_some() {
        "held"
    } else {
        "unsupported"
    };
    emit(
        printer,
        &json!({"ok": true, "started": true, "lock": lock_state}),
    );

    // 5-7. Switch, compose, and run until quit.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            emit(
                printer,
                &json!({"ok": false, "error": "runtime", "detail": error.to_string()}),
            );
            return EXIT_FAILURE;
        }
    };

    runtime.block_on(async move {
        let (quit_tx, mut quit_rx) = tokio::sync::watch::channel(false);
        let for_ms = args.for_ms;
        tokio::spawn(async move {
            wait_for_quit(for_ms).await;
            let _ = quit_tx.send(true);
        });

        if let Some(lock_arc) = lock {
            let on_settings = Arc::clone(&settings);
            let on = move || {
                on_settings.get("wire.enabled").as_deref() == Some("1")
                    || keeper_wire::debug_override()
            };

            let compose_settings = Arc::clone(&settings);
            let compose_sources = Arc::clone(&sources);
            let data_dir = args.data.clone();
            let compose = move || -> Result<Arc<dyn Dispatch>, String> {
                let listed: Vec<String> = compose_settings
                    .get("wire.domains")
                    .map(|raw| {
                        raw.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let followed = keeper_graph::client::follow(&compose_sources, &listed);
                let mut ok_dbs = Vec::new();
                for (db, outcome) in followed {
                    match outcome {
                        Ok(()) => ok_dbs.push(db),
                        Err(error) => {
                            emit(
                                printer,
                                &json!({"follow_failed": db, "detail": error.to_string()}),
                            );
                        }
                    }
                }
                emit(printer, &json!({"followed": ok_dbs}));

                let bindings = compose_sources
                    .iter()
                    .map(|source| DomainBinding {
                        server: "local".to_owned(),
                        database: source.database.clone(),
                        actor: source.actor.clone(),
                        client: source.client.clone(),
                        source: Ok(source.clone()),
                    })
                    .collect();
                let (runtime, _) =
                    crate::compose::build(crate::graph::serve_config(&data_dir), bindings)
                        .map_err(|error| error.to_string())?;
                Ok(Arc::new(runtime.wire_dispatcher()) as Arc<dyn Dispatch>)
            };

            keeper_wire::follow_switch(lock_arc, on, compose, Limits::default(), quit_rx).await;
        } else {
            while !*quit_rx.borrow() {
                if quit_rx.changed().await.is_err() {
                    break;
                }
            }
        }
    });

    emit(printer, &json!({"ok": true, "stopped": true}));
    EXIT_OK
}

async fn wait_for_quit(for_ms: Option<u64>) {
    match for_ms {
        Some(ms) => {
            tokio::select! {
                () = wait_for_signal() => {}
                () = tokio::time::sleep(Duration::from_millis(ms)) => {}
            }
        }
        None => wait_for_signal().await,
    }
}

async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut interrupt), Ok(mut terminate)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
    ) else {
        std::future::pending::<()>().await;
        return;
    };
    tokio::select! {
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
    }
}
