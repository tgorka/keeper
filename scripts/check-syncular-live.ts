// Live check of the syncular contract against a real Postgres.
//
// KEEPER_SYNC_TEST_PG_URL holds one or more space-separated connection URLs.
// Each logs in as a role whose search_path names the schema the contract is
// served from; one server serves one schema, so a URL per schema checks them
// all. For each URL this runs the server's startup twice — storage
// `migrate()` and `ensureSyncServerReady` over the generated schema — and
// fails unless:
//   - the first start creates every contract table in the role's schema and
//     none in `public`;
//   - the second start changes no table, column, index or the stored schema
//     marker;
//   - rows committed through the real push path come back from the
//     registered `sessionPage` query, newest first, page by page, and a page
//     key outside the actor's scopes is refused.
//
// Run against a throwaway database: it writes rows into partition
// `live-check`.
//
//   KEEPER_SYNC_TEST_PG_URL="postgres://role_a:…@host/db postgres://role_b:…@host/db" \
//     bun run check:syncular:live

import { decodeRemoteOperationResponse, encodeRemoteOperationRequest } from "@syncular/core";
import {
  ensureSyncServerReady,
  handleRemoteOperation,
  MemorySegmentStore,
  type PgExecutor,
  type PgQueryable,
  PostgresServerStorage,
  RemoteOperationRegistry,
  registerRemoteQuery,
  type ServerSchema,
  type SyncServerConfig,
  seedMutations,
} from "@syncular/server";
import { SQL } from "bun";
import { schema as generatedSchema } from "../syncular/syncular.generated.ts";
import { sessionPageQuery } from "../syncular/syncular.queries.ts";

const schema = generatedSchema as unknown as ServerSchema;
const PARTITION = "live-check";
const ACTOR = "live-check-person";
const CLIENT = "live-check-device";
const SESSION = `${CLIENT}.1`;
const PAGE = `${SESSION}/2026-10`;
const OTHER_PAGE = `${CLIENT}.2/2026-10`;
const FIRST_PAGE_CURSOR = { beforeMs: 9007199254740991n, beforeId: "" };

function fail(message: string): never {
  console.error(`check-syncular-live: ${message}`);
  process.exit(1);
}

/** Bun.SQL as syncular's PgExecutor. */
function executor(sql: SQL): PgExecutor {
  const over = (handle: { unsafe(text: string, values: unknown[]): PromiseLike<unknown> }) => ({
    async query<Row>(text: string, params?: readonly unknown[]) {
      const rows = (await handle.unsafe(text, params ? [...params] : [])) as Row[];
      return { rows, rowCount: rows.length };
    },
  });
  return {
    query: over(sql).query,
    transaction: <T>(fn: (client: PgQueryable) => Promise<T>) =>
      sql.begin((tx) => fn(over(tx))) as Promise<T>,
    close: () => sql.end(),
  };
}

function config(sql: SQL): SyncServerConfig {
  return {
    schema,
    storage: new PostgresServerStorage(executor(sql)),
    segments: new MemorySegmentStore(),
    resolveScopes: () => ({
      session: [SESSION],
      member: [ACTOR],
      page: [PAGE],
      trace: [PAGE],
      actor: [ACTOR],
    }),
  };
}

/** One server start: the storage's own tables, then the contract's. */
async function start(sql: SQL): Promise<SyncServerConfig> {
  const server = config(sql);
  await (server.storage as PostgresServerStorage).migrate();
  await ensureSyncServerReady(server);
  return server;
}

/** What a start may change: relations, columns, indexes and the marker. */
async function catalog(sql: SQL, schemaName: string): Promise<string> {
  const relations = await sql`
    SELECT c.relname, c.relkind, c.xmin::text AS xmin
      FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
     WHERE n.nspname = ${schemaName} AND c.relkind IN ('r', 'i')
     ORDER BY c.relname`;
  const columns = await sql`
    SELECT table_name, column_name, data_type, is_nullable, ordinal_position
      FROM information_schema.columns
     WHERE table_schema = ${schemaName}
     ORDER BY table_name, ordinal_position`;
  const indexes = await sql`
    SELECT indexname, indexdef FROM pg_indexes
     WHERE schemaname = ${schemaName} ORDER BY indexname`;
  const marker = await sql`SELECT schema_version, layouts FROM sync_schema_meta WHERE id = 1`;
  return JSON.stringify({ relations, columns, indexes, marker });
}

async function runSessionPage(
  server: SyncServerConfig,
  registry: RemoteOperationRegistry,
  params: Record<string, unknown>,
) {
  const bytes = await handleRemoteOperation(
    encodeRemoteOperationRequest({
      revision: 1,
      kind: "query",
      clientId: CLIENT,
      operationId: sessionPageQuery.id,
      params,
    }),
    { ...server, actorId: ACTOR, partition: PARTITION },
    registry,
  );
  return decodeRemoteOperationResponse(bytes);
}

async function checkOne(url: string): Promise<void> {
  const sql = new SQL(url);
  const [{ schema_name: schemaName, role }] = await sql`
    SELECT current_schema() AS schema_name, current_user AS role`;
  if (schemaName === null || schemaName === "public") {
    fail(`${role}: search_path must name the role's own schema, got ${schemaName}`);
  }
  const contract = schema.tables.map((table) => table.name);

  const first = await start(sql);
  const placed = await sql`
    SELECT table_schema, table_name FROM information_schema.tables
     WHERE table_name IN ${sql(contract)}`;
  const inOwn = placed.filter((row: { table_schema: string }) => row.table_schema === schemaName);
  const inPublic = placed.filter((row: { table_schema: string }) => row.table_schema === "public");
  if (inOwn.length !== contract.length) {
    fail(`${role}: ${inOwn.length} of ${contract.length} contract tables in ${schemaName}`);
  }
  if (inPublic.length !== 0) fail(`${role}: contract tables in public: ${inPublic.length}`);
  const afterFirst = await catalog(sql, schemaName);

  await start(sql);
  const afterSecond = await catalog(sql, schemaName);
  if (afterSecond !== afterFirst) fail(`${role}: the second start changed the catalog`);

  const now = 1792454400000; // 2026-10-20T00:00:00Z
  const authored = { author_actor: ACTOR, author_client: CLIENT };
  await seedMutations(first, { partition: PARTITION, actorId: ACTOR, clientId: CLIENT }, [
    {
      table: "sessions",
      op: "upsert",
      values: {
        id: SESSION,
        title: "live check",
        status: "open",
        agent_actor: "live-check-agent",
        created_by_actor: ACTOR,
        created_by_client: CLIENT,
        invite: "[]",
        archived: true,
        created_at_ms: now,
        last_activity_ms: now + 2,
        ...authored,
      },
    },
    ...[0, 1, 2].map((n) => ({
      table: "messages",
      op: "upsert" as const,
      values: {
        id: `01JLIVECHECK00000000000000${n}`,
        session_id: SESSION,
        page_key: PAGE,
        role: "person",
        body: `message ${n}`,
        created_at_ms: now + n,
        ...authored,
      },
    })),
  ]);

  const registry = new RemoteOperationRegistry([
    registerRemoteQuery(sessionPageQuery, { maxRows: 10_000, auth: { access: "scoped" } }),
  ]);
  const page1 = await runSessionPage(first, registry, {
    pageKey: PAGE,
    ...FIRST_PAGE_CURSOR,
    pageSize: 2n,
  });
  if (page1.kind !== "query") fail(`${role}: sessionPage failed: ${JSON.stringify(page1)}`);
  const bodies1 = page1.rows.map((row) => row.body);
  if (JSON.stringify(bodies1) !== JSON.stringify(["message 2", "message 1"])) {
    fail(`${role}: first page ${JSON.stringify(bodies1)}`);
  }
  const last = page1.rows[1] as { created_at_ms: number; id: string };
  const page2 = await runSessionPage(first, registry, {
    pageKey: PAGE,
    beforeMs: BigInt(last.created_at_ms),
    beforeId: last.id,
    pageSize: 2n,
  });
  if (page2.kind !== "query") fail(`${role}: sessionPage failed: ${JSON.stringify(page2)}`);
  const bodies2 = page2.rows.map((row) => row.body);
  if (JSON.stringify(bodies2) !== JSON.stringify(["message 0"])) {
    fail(`${role}: second page ${JSON.stringify(bodies2)}`);
  }
  const outside = await runSessionPage(first, registry, {
    pageKey: OTHER_PAGE,
    ...FIRST_PAGE_CURSOR,
    pageSize: 2n,
  });
  if (outside.kind !== "error" || outside.code !== "operation.forbidden") {
    fail(`${role}: a page outside the actor's scopes answered ${JSON.stringify(outside)}`);
  }

  await sql.end();
  console.log(
    `${role} → ${schemaName}: ${contract.length} tables there and none in public, second start a no-op, ` +
      `sessionPage paged ${bodies1.length}+${bodies2.length} rows, foreign page refused`,
  );
}

const urls = (process.env.KEEPER_SYNC_TEST_PG_URL ?? "").split(/\s+/).filter(Boolean);
if (urls.length === 0) fail("KEEPER_SYNC_TEST_PG_URL is not set");
for (const url of urls) await checkOne(url);
