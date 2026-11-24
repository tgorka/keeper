//! Test-only verbs (`keeper-cli debug ...`, story 115.3); hidden from `--help`.
//!
//! `debug transfer put|get` is a thin wrapper over `keeper_replica::transfer`:
//! it discovers the database's announced object store, builds `HttpGrants` and
//! `StoreTransfer`, and runs the transfer on the current thread. The bearer
//! token is always read from an environment variable and never printed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use keeper_ports::transfer::FileTransfer as _;
use keeper_ports::{ReplicaError, StoreSlot};
use keeper_replica::transfer::{announced_store, HttpGrants, StoreTransfer, TransferConfig};
use serde_json::{json, Value};

use crate::output::Printer;
use crate::{EXIT_CONFIG, EXIT_FAILURE, EXIT_OK};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Hidden debug subcommands.
#[derive(clap::Subcommand)]
pub enum Args {
    /// Upload or download a file through the database's object store.
    #[command(subcommand)]
    Transfer(Transfer),
    /// Test composition of the macOS app's start (story 123.3).
    AppStart(crate::app_start::Args),
}

/// File transfer subcommands.
#[derive(clap::Subcommand)]
pub enum Transfer {
    /// Upload a file to the database's object store.
    Put {
        #[command(flatten)]
        common: Common,
        /// Path to the file to upload.
        file: PathBuf,
        /// Optional MIME media type.
        #[arg(long)]
        media_type: Option<String>,
    },
    /// Download a file from the database's object store.
    Get {
        #[command(flatten)]
        common: Common,
        /// Content id (`sha256:<hex>`).
        #[arg(long)]
        content_id: String,
        /// Expected size in bytes.
        #[arg(long)]
        size: u64,
        /// Destination path.
        #[arg(long)]
        out: PathBuf,
    },
}

/// Arguments shared by `debug transfer put` and `get`.
#[derive(clap::Args)]
pub struct Common {
    /// Server root URL.
    #[arg(long)]
    pub url: String,
    /// Database name.
    #[arg(long)]
    pub database: String,
    /// Name of the environment variable holding the bearer token.
    #[arg(long)]
    pub token_env: String,
    /// Device-local directory for upload plans (default `<file>.plans` for put).
    #[arg(long)]
    pub plan_dir: Option<PathBuf>,
    /// Override the single-PUT threshold in bytes.
    #[arg(long, hide = true)]
    pub threshold: Option<u64>,
    /// Override the part size in bytes.
    #[arg(long, hide = true)]
    pub part_bytes: Option<u64>,
}

pub fn main(args: Args, printer: &Printer) -> u8 {
    match args {
        Args::Transfer(transfer) => run_transfer(transfer, printer),
        Args::AppStart(app_start) => crate::app_start::main(app_start, printer),
    }
}

fn run_transfer(cmd: Transfer, printer: &Printer) -> u8 {
    match cmd {
        Transfer::Put {
            common,
            file,
            media_type,
        } => {
            let plan_dir = common
                .plan_dir
                .clone()
                .unwrap_or_else(|| default_plan_dir(&file));
            let transfer = match build_transfer(&common, plan_dir, printer) {
                Ok(transfer) => transfer,
                Err(code) => return code,
            };
            match transfer.upload(&file, media_type.as_deref(), &mut |_| {}) {
                Ok(uploaded) => {
                    let answer = json!({
                        "ok": true,
                        "contentId": uploaded.content_id,
                        "size": uploaded.size,
                        "bytesSent": uploaded.bytes_sent,
                        "partsSent": uploaded.parts_sent,
                        "partsSkipped": uploaded.parts_skipped,
                    });
                    printer.answer(&answer, |_| {
                        format!(
                            "uploaded {} ({} bytes, {} sent, {} part(s) sent, {} skipped)",
                            uploaded.content_id,
                            uploaded.size,
                            uploaded.bytes_sent,
                            uploaded.parts_sent,
                            uploaded.parts_skipped,
                        )
                    });
                    EXIT_OK
                }
                Err(error) => fail_replica(printer, &error),
            }
        }
        Transfer::Get {
            common,
            content_id,
            size,
            out,
        } => {
            let plan_dir = common
                .plan_dir
                .clone()
                .unwrap_or_else(|| default_plan_dir(&out));
            let transfer = match build_transfer(&common, plan_dir, printer) {
                Ok(transfer) => transfer,
                Err(code) => return code,
            };
            match transfer.download(&content_id, size, &out, &mut |_| {}) {
                Ok(downloaded) => {
                    let answer = json!({
                        "ok": true,
                        "contentId": content_id,
                        "size": downloaded.size,
                        "bytesFetched": downloaded.bytes_fetched,
                        "grants": downloaded.grants,
                    });
                    printer.answer(&answer, |_| {
                        format!(
                            "downloaded {} ({} bytes, {} fetched, {} grant(s))",
                            content_id,
                            downloaded.size,
                            downloaded.bytes_fetched,
                            downloaded.grants,
                        )
                    });
                    EXIT_OK
                }
                Err(error) => fail_replica(printer, &error),
            }
        }
    }
}

fn build_transfer(
    common: &Common,
    plan_dir: PathBuf,
    printer: &Printer,
) -> Result<StoreTransfer, u8> {
    let url = common.url.trim().trim_end_matches('/');
    if url.is_empty() {
        return Err(fail(
            printer,
            "config.invalid_url",
            "--url must not be empty",
            EXIT_CONFIG,
        ));
    }
    if common.database.trim().is_empty() {
        return Err(fail(
            printer,
            "config.invalid_database",
            "--database must not be empty",
            EXIT_CONFIG,
        ));
    }
    if common.token_env.trim().is_empty() {
        return Err(fail(
            printer,
            "config.missing_token",
            "--token-env must not be empty",
            EXIT_CONFIG,
        ));
    }
    if matches!(common.part_bytes, Some(0)) {
        return Err(fail(
            printer,
            "config.invalid_part_bytes",
            "--part-bytes must be positive",
            EXIT_CONFIG,
        ));
    }
    let token = match std::env::var(&common.token_env) {
        Ok(value) if !value.is_empty() => value,
        _ => {
            return Err(fail(
                printer,
                "config.missing_token",
                &format!(
                    "environment variable {} is not set or empty",
                    common.token_env
                ),
                EXIT_CONFIG,
            ));
        }
    };

    let store = announced_store(url, &common.database, &token, REQUEST_TIMEOUT)
        .map_err(|error| fail_replica(printer, &error))?;
    let slot = StoreSlot::default();
    slot.set(store);

    let base = format!("{url}/{}", common.database);
    let grants = Arc::new(
        HttpGrants::new(&base, &token, REQUEST_TIMEOUT)
            .map_err(|error| fail_replica(printer, &error))?,
    );

    let mut config = TransferConfig::contract(plan_dir);
    if let Some(threshold) = common.threshold {
        config.threshold = threshold;
    }
    if let Some(part_bytes) = common.part_bytes {
        config.part_bytes = part_bytes;
    }

    StoreTransfer::new(grants, &base, slot, config).map_err(|error| fail_replica(printer, &error))
}

fn default_plan_dir(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".plans");
    PathBuf::from(name)
}

fn fail_replica(printer: &Printer, error: &ReplicaError) -> u8 {
    let (code, message): (&str, String) = match error {
        ReplicaError::Remote { code, message, .. }
        | ReplicaError::Rejected { code, message }
        | ReplicaError::Unheard { code, message, .. } => (code.as_str(), message.clone()),
        ReplicaError::Client(message) => ("client.error", message.clone()),
        ReplicaError::NotSyncing => ("client.not_syncing", error.to_string()),
        ReplicaError::Closed => ("client.closed", error.to_string()),
    };
    fail(printer, code, &message, EXIT_FAILURE)
}

fn fail(printer: &Printer, code: &str, message: &str, exit: u8) -> u8 {
    let answer: Value = json!({
        "ok": false,
        "code": code,
        "error": message,
    });
    printer.answer(&answer, |_| format!("{code}: {message}"));
    exit
}
