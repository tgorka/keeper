# Keeper companion stack

A self-hosted Matrix homeserver + bridge stack that pairs with the
[keeper](../../README.md) client: **Synapse** plus the **mautrix** bridges for
Telegram, WhatsApp and Signal, wired so keeper's native Bridges UI can discover
and log in to them.

This is configuration only. Keeper is a client and ships **no hosted
services** — you operate this stack yourself, on your own hardware, under your
own accounts and ToS exposure (see
[docs/constraints-and-limitations.md](../../docs/constraints-and-limitations.md)).

## Why Synapse (and not conduwuit/continuwuity)

- **Simplified Sliding Sync (MSC4186) is native.** Keeper *requires* SSS and
  rejects homeservers without it at login. Current Synapse implements MSC4186
  in-tree — no sliding-sync proxy, no experimental fork.
- **mautrix compatibility.** The mautrix bridges (bridgev2) and their
  provisioning APIs are developed and tested against Synapse first; appservice
  registration, double-puppeting and the provisioning flows keeper's Bridges UI
  depends on are the well-trodden path here.

## Licensing note

Synapse and the mautrix bridges are **AGPL-3.0** software. They run as
separate services from their own upstream Docker images and communicate with
keeper only over network APIs. This repository ships configuration files only —
no AGPL code is copied or linked, and keeper itself remains Apache-2.0.

## Federation is off

This stack is a **private homeserver**: it serves only the client-server API on
port 8008. No federation listener is configured, port 8448 is not exposed, and
no federation TLS/signing-key setup is needed. Users on this server can talk to
each other and to the bridges, not to other Matrix servers. (To enable
federation later you would add a federation listener and publish 8448 —
deliberately out of scope here.)

## Prerequisites

- Docker with the compose plugin.
- A postgres instance (any recent version). If you don't have one, uncomment
  the `postgres` service in `docker-compose.yml`.
- A DNS name (or Tailscale MagicDNS name / LAN hostname) for `SERVER_NAME`.

## Quickstart

All commands run from this directory (`deploy/companion-stack/`).

### 1. Environment

```sh
cp .env.example .env   # then edit: SERVER_NAME, PUBLIC_URL, postgres, secret
```

Use 1Password references for secrets if you can (see the header of
`.env.example` and this repo's `/.env.1p` pattern): run everything through
`op run --env-file=.env -- docker compose ...`.

### 2. Create the databases

Synapse requires `C` locale with `UTF8` encoding, so create from `template0`.
Each bridge gets its own database. On your postgres (adjust the role name and
password source to taste):

```sql
CREATE ROLE synapse LOGIN PASSWORD '...';
CREATE DATABASE synapse    OWNER synapse TEMPLATE template0 ENCODING 'UTF8' LC_COLLATE 'C' LC_CTYPE 'C';
CREATE DATABASE mxtelegram OWNER synapse TEMPLATE template0 ENCODING 'UTF8' LC_COLLATE 'C' LC_CTYPE 'C';
CREATE DATABASE mxwhatsapp OWNER synapse TEMPLATE template0 ENCODING 'UTF8' LC_COLLATE 'C' LC_CTYPE 'C';
CREATE DATABASE mxsignal   OWNER synapse TEMPLATE template0 ENCODING 'UTF8' LC_COLLATE 'C' LC_CTYPE 'C';
```

### 3. Generate the Synapse config

```sh
docker compose run --rm synapse generate
```

This writes `data/synapse/homeserver.yaml` (+ log config and signing key).
Edit `data/synapse/homeserver.yaml`:

- Replace the sqlite `database:` block with postgres:

  ```yaml
  database:
    name: psycopg2
    args:
      user: synapse
      password: "..."
      dbname: synapse
      host: host.docker.internal   # your POSTGRES_HOST
      port: 5432
      cp_min: 5
      cp_max: 10
  ```

- `public_baseurl: <PUBLIC_URL>`
- `enable_registration: false`
- `registration_shared_secret: "<REGISTRATION_SHARED_SECRET from .env>"`
- Leave only the client listener (the generated config has no federation
  listener when you don't add one; do not add `federation` to any
  `listeners.resources`).
- Prepare the appservice list for step 5:

  ```yaml
  app_service_config_files:
    - /data/appservices/telegram.yaml
    - /data/appservices/whatsapp.yaml
    - /data/appservices/signal.yaml
  ```

  (Comment the list out until the files exist, or Synapse will refuse to
  start.)

Then start Synapse and check it:

```sh
docker compose up -d synapse
curl -s http://localhost:8008/_matrix/client/versions | head -c 300
```

### 4. Generate each bridge's config + registration

The mautrix bridgev2 images follow the same three-run flow (shown for
telegram; repeat for `mautrix-whatsapp` and `mautrix-signal`):

```sh
# Run 1: writes an example config to data/telegram/config.yaml, then exits.
docker compose run --rm mautrix-telegram

# Edit data/telegram/config.yaml:
#   homeserver:
#     address: http://synapse:8008        # compose-internal address
#     domain: <SERVER_NAME>
#   appservice:
#     address: http://mautrix-telegram:<port>  # compose service name + the
#                                              # port already in the generated config
#   database:
#     type: postgres
#     uri: postgres://synapse:...@host.docker.internal:5432/mxtelegram?sslmode=disable
#   provisioning:                         # REQUIRED for keeper's Bridges UI
#     prefix: /_matrix/provision
#     shared_secret: generate
#   permissions:
#     "<SERVER_NAME>": user               # or "@you:<SERVER_NAME>": admin

# Run 2: validates config, writes data/telegram/registration.yaml, then exits.
docker compose run --rm mautrix-telegram
```

Bridge databases: `mxtelegram`, `mxwhatsapp`, `mxsignal` (from step 2).

### 5. Register the bridges with Synapse

```sh
mkdir -p data/synapse/appservices
cp data/telegram/registration.yaml data/synapse/appservices/telegram.yaml
cp data/whatsapp/registration.yaml data/synapse/appservices/whatsapp.yaml
cp data/signal/registration.yaml   data/synapse/appservices/signal.yaml
```

Uncomment `app_service_config_files` in `homeserver.yaml`, then:

```sh
docker compose up -d          # restarts synapse, starts all three bridges
docker compose ps             # everything should be running/healthy
```

### 6. Create your first user

Registration is disabled publicly; use the shared secret via the CLI:

```sh
docker compose exec synapse register_new_matrix_user \
  -u <username> -p '<strong password>' --no-admin \
  -k "<REGISTRATION_SHARED_SECRET>" http://localhost:8008
```

## Connecting from keeper

1. Open keeper → add account → homeserver URL = your `PUBLIC_URL`
   (e.g. `http://matrix.example.com:8008`).
2. Log in with `@<username>:<SERVER_NAME>` and the password from step 6.
   Keeper verifies Simplified Sliding Sync support at login — current Synapse
   passes this natively.

### Bridge login happens from keeper

You do **not** chat with bridge bots by hand. In keeper: **Bridges → Connect**
on the network you want. Keeper discovers the bridges through
`/_matrix/client/v3/thirdparty/protocols` and the bridge bots
(`@telegrambot:…`, `@whatsappbot:…`, `@signalbot:…`), then drives the bridge's
provisioning API for login — QR code (WhatsApp/Signal) or phone code
(Telegram) shown directly in keeper's UI. This is why `provisioning:` must be
enabled in each bridge config (step 4).

## Backups

- **Postgres**: dump all four databases (`synapse`, `mxtelegram`,
  `mxwhatsapp`, `mxsignal`), e.g.
  `pg_dump -Fc -U synapse <db> > <db>.dump` on a schedule.
- **Media**: `data/synapse/media_store/` (uploaded/bridged attachments live
  here, not in postgres).
- **Keys/registrations**: `data/synapse/*.signing.key` and the bridge
  `registration.yaml` files — tiny, but losing them means re-registering.
