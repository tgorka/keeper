//! The command line: `init`, `login`, `agents list`, `run`, `status`.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use keeper_agent::headless::{self, harden_process, HeadlessError, SecretMap, APP_DIR};
use keeper_core::agents::agentd::{AgentdConfig, FILE_NAME};
use keeper_sync::xdg::{SecretStore, XdgDirs};

/// Success.
pub const EXIT_OK: u8 = 0;
/// Something failed while running.
pub const EXIT_FAILURE: u8 = 1;
/// The configuration is missing or refused. A missing `git` is `3`
/// (`HeadlessError::exit_code`), syncd's numbers throughout.
pub const EXIT_CONFIG: u8 = 2;

#[derive(Debug, Parser)]
#[command(
    name = "keeper-agentd",
    version,
    about = "keeper's agents on a Linux server"
)]
pub struct Cli {
    /// The configuration file.
    #[arg(long, global = true, env = "KEEPER_AGENTD_CONFIG")]
    pub config: Option<PathBuf>,
    /// More logging: -v debug, -vv trace.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write the agentd.toml skeleton if there is none, and create the
    /// principal's control room once a copy is signed in.
    Init,
    /// Sign one agent's copy in to the homeserver as this host's device.
    Login {
        /// `<drive>/<agent>`, as `[[agents]]` names it.
        agent: String,
        /// Read the password from this secret instead of asking for it.
        #[arg(long)]
        password_credential: Option<String>,
    },
    /// The agents zones, their agents and their copies.
    Agents {
        #[command(subcommand)]
        command: AgentsCommand,
    },
    /// Serve the agents until SIGTERM.
    Run,
    /// What this host mounts, serves and offers.
    Status {
        /// `<drive>/<session path>`: what that session's agent was told.
        #[arg(long)]
        session: Option<String>,
        /// With `--session`: compose without asking the provider which tools
        /// its model supports, so `status` reaches nothing.
        #[arg(long, requires = "session")]
        no_probe: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentsCommand {
    /// Every zone and home with its verdict.
    List,
}

/// Why a verb failed, with its exit code.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Config(String),
    #[error(transparent)]
    Headless(#[from] HeadlessError),
    #[error("{0}")]
    Failure(String),
}

impl CliError {
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::Config(_) => EXIT_CONFIG,
            CliError::Headless(error) => error.exit_code(),
            CliError::Failure(_) => EXIT_FAILURE,
        }
    }
}

/// Everything a verb works with.
pub struct Host {
    pub dirs: XdgDirs,
    pub config_path: PathBuf,
}

impl Host {
    /// Read and parse `agentd.toml`.
    pub fn config(&self) -> Result<AgentdConfig, CliError> {
        let text = std::fs::read_to_string(&self.config_path).map_err(|error| {
            CliError::Config(format!(
                "{} cannot be read: {error}. Run `keeper-agentd init` to write one.",
                self.config_path.display()
            ))
        })?;
        AgentdConfig::parse(&text).map_err(|refusal| {
            CliError::Config(format!(
                "{} is refused: {}",
                self.config_path.display(),
                refusal.sentence()
            ))
        })
    }

    /// Read every secret `config` names, scrub the secret variables and
    /// become non-dumpable — before any thread exists (S-07).
    pub fn harden(&self, config: &AgentdConfig) -> Result<Arc<SecretMap>, CliError> {
        let mut store: SecretStore = headless::secret_store(&self.dirs);
        let needed: Vec<&str> = config.secrets().into_iter().map(|s| s.name()).collect();
        harden_process(&mut store, &needed).map_err(|error| CliError::Config(error.to_string()))?;
        Ok(Arc::new(SecretMap::new(store)))
    }
}

/// The multi-thread runtime, built only after the hardening.
pub fn runtime() -> Result<tokio::runtime::Runtime, CliError> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::Failure(format!("the async runtime could not start: {error}")))
}

/// The default configuration path: `$XDG_CONFIG_HOME/keeper-agentd/agentd.toml`.
pub fn default_config(dirs: &XdgDirs) -> PathBuf {
    dirs.config.join(FILE_NAME)
}

/// Run one verb; its exit code.
pub fn run(cli: Cli) -> u8 {
    let dirs = match XdgDirs::resolve(APP_DIR) {
        Ok(dirs) => dirs,
        Err(error) => {
            tracing::error!(%error, "keeper-agentd: the XDG directories cannot be used");
            return EXIT_CONFIG;
        }
    };
    let host = Host {
        config_path: cli.config.clone().unwrap_or_else(|| default_config(&dirs)),
        dirs,
    };
    let result = match cli.command {
        Command::Init => crate::init::run(&host),
        Command::Login {
            agent,
            password_credential,
        } => crate::login::run(&host, &agent, password_credential.as_deref()),
        Command::Agents {
            command: AgentsCommand::List,
        } => crate::report::agents_list(&host),
        Command::Run => serve(&host),
        Command::Status { session, no_probe } => {
            crate::report::status(&host, session.as_deref(), !no_probe)
        }
    };
    match result {
        Ok(()) => EXIT_OK,
        Err(error) => {
            let code = error.exit_code();
            tracing::error!(error = %error, exit = code, "keeper-agentd failed");
            code
        }
    }
}

/// `run`: serve until SIGTERM or SIGINT.
fn serve(host: &Host) -> Result<(), CliError> {
    let config = host.config()?;
    let secrets = host.harden(&config)?;
    let dirs = XdgDirs {
        config: host.dirs.config.clone(),
        data: host.dirs.data.clone(),
        state: host.dirs.state.clone(),
    };
    runtime()?.block_on(async move {
        let (stop, shutdown) = tokio::sync::watch::channel(false);
        let signals = tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let (Ok(mut term), Ok(mut int)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
            ) else {
                tracing::error!("agentd: the signal handlers could not be installed");
                return;
            };
            let name = tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = int.recv() => "SIGINT",
            };
            tracing::info!(signal = name, "agentd: stopping");
            let _ = stop.send(true);
        });
        let ran = keeper_agent::runtime::run(config, dirs, secrets, shutdown).await;
        signals.abort();
        ran.map_err(CliError::from)
    })
}

/// `<drive>/<rest>` split at its first `/`.
pub fn split_ref(text: &str) -> Result<(&str, &str), CliError> {
    text.split_once('/')
        .filter(|(drive, rest)| !drive.is_empty() && !rest.is_empty())
        .ok_or_else(|| CliError::Config(format!("\"{text}\" is not <drive>/<name>")))
}
