-- Settings as rows a person owns, and a domain's shared configuration, schema version 11
-- (the README's *Settings*; story 121.1). 0001 through 0009 are never re-opened.
--
-- `settings_values` and `settings_devices` live in a person's personal domain under
-- `settings_owner:{settings_owner}` on `owner_actor`, which the server allows to that person for
-- `[self]` in their own personal domain only, and never to an agent instance, a tool or the
-- server's own client. They take no `actor:{person}` scope: an agent holds the `actor` of every
-- person it serves. Who may write each row, the travel class of each key, the refused values and
-- the adoption rule are `settings.vectors.json` and `settings.travel.json`.

-- One record: a preference, an account pin, an offer or a drive's roles. `id` is
-- `<device>/<key>`, where `device` is `*` for a record of the person (every device) or the
-- `device_id` of the `settings_devices` row whose bound client alone writes it. `value` is the
-- stored string; a value longer than `limits.json`'s `settingValueMaxBytes` is `value_blob`
-- instead (a third-class blob in the conversation key space, read only by the owning person's own
-- device clients), and exactly one of the two is present. `revision` is the writer's count of
-- its own changes to the row, `updated_by_client` the client that made the last one.
CREATE TABLE settings_values (
  id TEXT PRIMARY KEY,
  owner_actor TEXT NOT NULL,
  device TEXT NOT NULL,
  key TEXT NOT NULL,
  value TEXT,
  value_blob BLOB_REF,
  revision INTEGER NOT NULL,
  updated_by_client TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX settings_values_owner_device ON settings_values (owner_actor, device, key);

-- One device of the person (D-27): `device_id` is the device's own ULID, `client_id` the client
-- the server bound the record to — the client that created it, moved only by an adoption.
-- `machine` is the fingerprint keeper computes (the SHA-256 of the operating system's machine id
-- with the sign-in's `sub`, hex), absent for a device that has none. The name follows the device;
-- the host name a client reports is never its identity. No restore gate: "restored once" is
-- `account.<id>.restored` in the install's own settings table.
CREATE TABLE settings_devices (
  device_id TEXT PRIMARY KEY,
  owner_actor TEXT NOT NULL,
  client_id TEXT NOT NULL,
  name TEXT NOT NULL,
  device_class TEXT NOT NULL,
  platform TEXT NOT NULL,
  version TEXT NOT NULL,
  machine TEXT,
  updated_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX settings_devices_owner ON settings_devices (owner_actor);
CREATE INDEX settings_devices_client ON settings_devices (client_id);

-- dir: a domain's shared configuration, read by every member through the directory unit. In the
-- `makidrive` domain it is the organisation's (the setting seeds, `template/…`), written only on
-- the operator's admin path; in any other domain it is that domain's own shared settings, written
-- on the admin path or by a person holding `owner` there. Never by an agent or a tool, and no key
-- or value names a domain. `value` is JSON within `settingValueMaxBytes`; `value_blob` holds a
-- larger one. `issuer` is who wrote it: `admin` or the owner's actor id.
CREATE TABLE config_values (
  key TEXT PRIMARY KEY,
  dir TEXT NOT NULL,
  value JSON,
  value_blob BLOB_REF,
  revision INTEGER NOT NULL,
  issuer TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
