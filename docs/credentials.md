# Credentials

Keeper is an open-source repository. **No credentials, tokens, or homeserver session data are
ever committed.** Development credentials live in 1Password and are read with the
[`op` CLI](https://developer.1password.com/docs/cli/).

## Dev test accounts

A 1Password item named **`keeper dev matrix`** (vault: `Private`) holds the Matrix test
account used for local development and manual testing:

| Field        | Meaning                                             |
| ------------ | --------------------------------------------------- |
| `homeserver` | Homeserver URL, e.g. `https://matrix.example.org`   |
| `username`   | Full Matrix user id, e.g. `@keeper-dev:example.org` |
| `password`   | Account password                                    |

Read values ad hoc:

```sh
op item get "keeper dev matrix" --fields label=homeserver
op item get "keeper dev matrix" --fields label=username
op item get "keeper dev matrix" --reveal --fields label=password
```

Or inject into a dev shell via `op run`:

```sh
op run --env-file=.env.1p -- bun run tauri dev
```

with `.env.1p` (committed, contains only `op://` references, no secrets):

```
KEEPER_DEV_HOMESERVER=op://Private/keeper dev matrix/homeserver
KEEPER_DEV_USERNAME=op://Private/keeper dev matrix/username
KEEPER_DEV_PASSWORD=op://Private/keeper dev matrix/password
```

## Beeper accounts

For testing against Beeper, log in inside the app itself (email + code, or app password).
Session tokens are stored by the app in the OS keychain — never in the repo.

## Runtime storage

The app stores Matrix session material (access tokens, E2EE keys) in:

- macOS Keychain for secrets (via Tauri/keyring)
- the app data directory (`~/Library/Application Support/dev.tgorka.keeper/`) for the
  encrypted matrix-rust-sdk store

Both are outside the repository.
