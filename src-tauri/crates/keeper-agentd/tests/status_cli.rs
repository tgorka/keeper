//! `keeper-agentd status` from the command line over a running host's
//! status file (96.2 #3, #5): what each MCP server an agent names said of
//! itself, as the host wrote it, read back through the CLI.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;

use keeper_agent::mcp::McpServers;
use keeper_core::agents::agentd::AgentdConfig;
use rmcp::model::{
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
const BOT: &str = "bot:openai:https://provider.example:8452#m";
const TGORKA: &str = "@tgorka:example.org";

/// Lists `echo`, which it offers, and `get file`, a name that cannot travel.
#[derive(Clone)]
struct Notes;

impl rmcp::ServerHandler for Notes {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult::with_all_items(
            ["echo", "get file"]
                .into_iter()
                .map(|name| Tool::new(name, "A tool.", Arc::new(serde_json::Map::new())))
                .collect(),
        ))
    }
}

async fn notes() -> String {
    let service = StreamableHttpService::new(
        || Ok(Notes),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let router = axum::Router::new().nest_service("/mcp", service);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{address}/mcp")
}

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("keeper-agentd")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The `mcp <name>` line of `said`, and the lines indented under it.
fn section(said: &str, name: &str) -> Vec<String> {
    let mut lines = said
        .lines()
        .skip_while(|line| !line.trim_start().starts_with(&format!("mcp {name}:")));
    let Some(head) = lines.next() else {
        return Vec::new();
    };
    std::iter::once(head)
        .chain(lines.take_while(|line| line.starts_with("    ")))
        .map(str::to_owned)
        .collect()
}

/// 96.2 #3, #5: `status` names each server Nixi's `[tools].mcp` lists as
/// the running host's status file says it — the one that answers with how
/// many tools it offers and the tool it does not offer with that tool's
/// own reason, the silent one with its own reason, and the one the host
/// does not name with neither server's.
#[tokio::test(flavor = "multi_thread")]
async fn status_says_what_each_mcp_server_said() {
    let root = tempfile::tempdir().expect("tempdir");
    let home = root.path().join("home");
    let checkout: PathBuf = home.join("data/keeper-agentd/drives/tgdrive");
    std::fs::create_dir_all(&checkout).expect("checkout");
    let into = checkout.to_string_lossy().into_owned();
    let seeded = run(
        &home,
        &[
            "agents",
            "init",
            "tgdrive",
            "--principal",
            "tgorka",
            "--owner",
            TGORKA,
            "--reader",
            TGORKA,
            "--bot",
            BOT,
            "--with",
            "nixi",
            "--into",
            &into,
        ],
    );
    assert!(seeded.status.success(), "{}", text(&seeded));
    let toml = checkout.join("80-agents/nixi/agent.toml");
    let agent = std::fs::read_to_string(&toml).expect("agent.toml");
    std::fs::write(
        &toml,
        agent.replace(
            "drives = [\"tgdrive\", \"neuradrive\"]\n",
            "drives = [\"tgdrive\", \"neuradrive\"]\nmcp = [\"notes\", \"gone\", \"absent\"]\n",
        ),
    )
    .expect("agent.toml");

    let url = notes().await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let nobody = format!("http://{}/mcp", closed.local_addr().expect("address"));
    drop(closed);
    let config = format!(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\n\n[homeserver]\nurl = \"https://m.example.org\"\n\n[[drives]]\nid = \"tgdrive\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\n\n[[agents]]\ndrive = \"tgdrive\"\nids = [\"nixi\"]\n\n[[mcp]]\nname = \"notes\"\nurl = \"{url}\"\n\n[[mcp]]\nname = \"gone\"\nurl = \"{nobody}\"\n",
        root.path().join("tgdrive.git").display()
    );
    std::fs::create_dir_all(home.join("config/keeper-agentd")).expect("config");
    std::fs::write(home.join("config/keeper-agentd/agentd.toml"), &config).expect("agentd.toml");

    // What the running host writes of its servers.
    let parsed = AgentdConfig::parse(&config).expect("parses");
    let servers = McpServers::new(parsed.mcp.into_iter().map(|entry| (entry, None)).collect());
    servers.refresh().await;
    let heard = servers.status();
    let gone_why = heard[1]["why"].as_str().expect("why").to_owned();
    let not_offered_why = heard[0]["not_offered"][0]["why"]
        .as_str()
        .expect("why")
        .to_owned();
    std::fs::create_dir_all(home.join("state/keeper-agentd")).expect("state");
    std::fs::write(
        home.join("state/keeper-agentd/status.json"),
        serde_json::json!({"updated_at": "2026-10-08T00:00:00Z", "mcp": heard}).to_string(),
    )
    .expect("status.json");

    let out = tokio::task::spawn_blocking(move || run(&home, &["status"]))
        .await
        .expect("status");
    let said = text(&out);
    assert!(out.status.success(), "{said}");

    let notes = section(&said, "notes");
    assert_eq!(notes.len(), 2, "{said}");
    assert!(notes[0].contains('1'), "one tool offered: {said}");
    assert!(
        notes[1].contains("get file") && notes[1].contains(&not_offered_why),
        "{said}"
    );
    assert!(!notes.iter().any(|line| line.contains(&gone_why)), "{said}");

    let gone = section(&said, "gone");
    assert_eq!(gone.len(), 1, "{said}");
    assert!(gone[0].contains(&gone_why), "{said}");

    let absent = section(&said, "absent");
    assert_eq!(absent.len(), 1, "{said}");
    assert!(
        !absent[0].contains(&gone_why) && !absent[0].contains(&not_offered_why),
        "{said}"
    );
}
