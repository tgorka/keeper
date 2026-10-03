//! `keeper-agentd login <drive>/<agent>`: one agent's copy on this host.

use std::io::{BufRead, Write};

use keeper_agent::headless::SecretMap;
use keeper_agent::runtime::{check_out_missing, inspect};
use keeper_core::agents::matrix::{self, AgentClient};
use keeper_core::auth::StoredSession;
use matrix_sdk::ruma::OwnedUserId;

use crate::cli::{runtime, split_ref, CliError, Host};

/// The device id of the copy's stored session, so a new sign-in reuses it
/// and the agent's user never collects stale devices.
pub fn stored_device(secrets: &SecretMap, user: &str) -> Option<String> {
    let user = matrix_sdk_user(user)?;
    let json = secrets.get(&matrix::session_key(&user)).ok().flatten()?;
    matrix::device_of_session(&json)
}

fn matrix_sdk_user(user: &str) -> Option<OwnedUserId> {
    OwnedUserId::try_from(user).ok()
}

/// Read a line from the terminal with its echo off.
fn prompt_without_echo(prompt: &str) -> Result<String, CliError> {
    use rustix::termios::{tcgetattr, tcsetattr, LocalModes, OptionalActions};

    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|error| {
            CliError::Config(format!(
                "there is no terminal to ask for the password ({error}); pass --password-credential <name>"
            ))
        })?;
    let saved = tcgetattr(&tty).map_err(|error| CliError::Failure(error.to_string()))?;
    let mut quiet = saved.clone();
    quiet.local_modes.remove(LocalModes::ECHO);
    tcsetattr(&tty, OptionalActions::Now, &quiet)
        .map_err(|error| CliError::Failure(error.to_string()))?;
    let read = (|| {
        let mut writer = &tty;
        writer.write_all(prompt.as_bytes())?;
        writer.flush()?;
        let mut line = String::new();
        std::io::BufReader::new(&tty).read_line(&mut line)?;
        writer.write_all(b"\n")?;
        Ok::<_, std::io::Error>(line)
    })();
    // Echo comes back whatever the read did.
    let restored = tcsetattr(&tty, OptionalActions::Now, &saved);
    let line = read.map_err(|error| CliError::Failure(error.to_string()))?;
    restored.map_err(|error| CliError::Failure(error.to_string()))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// `login`.
pub fn run(host: &Host, agent: &str, password_credential: Option<&str>) -> Result<(), CliError> {
    let (drive, id) = split_ref(agent)?;
    let config = host.config()?;
    let secrets = host.harden(&config)?;
    let runtime = runtime()?;
    runtime.block_on(check_out_missing(&config, &host.dirs, &secrets))?;
    let inspection = inspect(&config, &host.dirs);
    let home = inspection
        .hosted
        .iter()
        .find(|home| home.config.drive == drive && home.config.id == id)
        .ok_or_else(|| {
            CliError::Config(format!(
                "{agent} is not an agent this host serves: name it in [[agents]] and check `keeper-agentd agents list`"
            ))
        })?;
    let user = home.config.matrix_user.clone();
    let password = match password_credential {
        Some(name) => secrets
            .store()
            .get(name)
            .map_err(|error| CliError::Config(error.to_string()))?
            .ok_or_else(|| CliError::Config(format!("secret:{name} is not set")))?,
        None => prompt_without_echo(&format!("Password for {user}: "))?,
    };
    let device = stored_device(&secrets, user.as_str());
    let store = matrix::store_dir(&host.dirs.data, &user);
    let stored = secrets
        .get(&matrix::passphrase_key(&user))
        .map_err(|error| CliError::Config(error.to_string()))?;
    let passphrase = matrix::sign_in_passphrase(stored, &store)
        .map_err(|error| CliError::Failure(error.to_string()))?;
    let display = format!("{}@{}", home.config.id, config.host);
    let session: StoredSession = runtime.block_on(async {
        let client = AgentClient::open(&config.homeserver.url.normalized, &store, &passphrase)
            .await
            .map_err(|error| CliError::Failure(error.to_string()))?;
        client
            .login(user.as_str(), &password, device.as_deref(), &display)
            .await
            .map_err(|error| CliError::Failure(error.to_string()))
    })?;
    let json = session
        .to_json()
        .map_err(|error| CliError::Failure(error.to_string()))?;
    let set = |key: String, value: &str| {
        secrets
            .set(&key, value)
            .map_err(|error| CliError::Failure(error.to_string()))
    };
    set(matrix::passphrase_key(&user), &passphrase)?;
    set(matrix::session_key(&user), &json)?;
    let now = stored_device(&secrets, user.as_str()).unwrap_or_default();
    match device {
        Some(old) if old == now => println!("Signed {user} in again as device {now} ({display})."),
        _ => println!("Signed {user} in as device {now} ({display})."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use keeper_agent::headless::SECRET_ENV_PREFIX;
    use keeper_sync::xdg::SecretStore;

    use super::*;

    /// The device a later login passes is the stored session's own, in
    /// either session shape; with no session there is none to reuse.
    #[test]
    fn login_reuses_the_device_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let secrets = SecretMap::new(SecretStore::new(SECRET_ENV_PREFIX, dir.path().join("s")));
        let user = "@nixi:example.org";
        assert_eq!(stored_device(&secrets, user), None);
        let key = matrix::session_key(&matrix_sdk_user(user).expect("user"));
        secrets
            .set(
                &key,
                r#"{"kind":"Password","user_id":"@nixi:example.org","device_id":"ELECTRA1","access_token":"t"}"#,
            )
            .expect("store");
        assert_eq!(stored_device(&secrets, user).as_deref(), Some("ELECTRA1"));
        let parsed = StoredSession::from_json(&secrets.get(&key).expect("get").expect("set"));
        assert!(parsed.is_ok(), "the stored shape is the one restore reads");
        secrets
            .set(
                &key,
                r#"{"kind":"Oauth","client_id":"c","user":{"meta":{"user_id":"@nixi:example.org","device_id":"ELECTRA2"},"tokens":{"access_token":"t"}}}"#,
            )
            .expect("store");
        assert_eq!(stored_device(&secrets, user).as_deref(), Some("ELECTRA2"));
    }
}
