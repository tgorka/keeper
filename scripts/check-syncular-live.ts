// Live check of the syncular contract against a real Postgres.
//
// KEEPER_SYNC_TEST_PG_URL holds one or more space-separated connection URLs.
// Each logs in as a role whose search_path names the schema the contract is
// served from; one server serves one schema, so a URL per schema checks them
// all. For each URL this runs the server's startup twice — storage
// `migrate()` and `ensureSyncServerReady` over the generated schema — and
// fails unless:
//   - the schema is empty before the first start, and the first start puts
//     every contract table in it and none in `public`;
//   - the second start changes no table, column, index or the stored schema
//     marker;
//   - rows this run commits through the real push path (fresh partition,
//     ids and commit ids, so nothing is a cached replay) come back from the
//     registered `sessionPage` query, newest first, page by page, and a page
//     key outside the actor's scopes is refused;
//   - an agent instance's two memory subscriptions, as the generated
//     templates request them, receive its own rows and its family peers'
//     rows and never another family's;
//   - the graph subscriptions, as the generated templates request them,
//     receive one graph's rows only under that graph's `kg_graph` value and
//     a client's receipts only under its own `kg_client` value, and a
//     caller holding every conversation scope but no graph scope receives
//     no graph row at all.
//
// Point it at a fresh, throwaway database: a schema that already holds
// tables is refused.
//
//   KEEPER_SYNC_TEST_PG_URL="postgres://role_a:…@host/db postgres://role_b:…@host/db" \
//     bun run check:syncular:live

import { decodeRemoteOperationResponse, encodeRemoteOperationRequest } from "@syncular/core";
import {
  computeEffective,
  ensureSyncServerReady,
  handleRemoteOperation,
  MemorySegmentStore,
  type PgExecutor,
  type PgQueryable,
  PostgresServerStorage,
  RemoteOperationRegistry,
  registerRemoteQuery,
  type ScopeMap,
  type ServerSchema,
  type SyncServerConfig,
  seedMutations,
} from "@syncular/server";
import { SQL } from "bun";
import {
  agentMemoryFamilySubscription,
  agentMemoryOwnSubscription,
  schema as generatedSchema,
  kgEdgesSubscription,
  kgEventOpsSubscription,
  kgEventsSubscription,
  kgHeadSubscription,
  kgNodesSubscription,
  kgNodeTagsSubscription,
  kgReceiptsSubscription,
  kgTagsSubscription,
} from "../syncular/syncular.generated.ts";
import { sessionPageQuery } from "../syncular/syncular.queries.ts";

const schema = generatedSchema as unknown as ServerSchema;
const RUN = crypto.randomUUID().replaceAll("-", "");
const PARTITION = `live-check-${RUN}`;
const ACTOR = `person-${RUN}`;
const CLIENT = `device-${RUN}`;
const SESSION = `${CLIENT}.1`;
const PAGE = `${SESSION}/2026-10`;
const OTHER_PAGE = `${CLIENT}.2/2026-10`;
const FIRST_PAGE_CURSOR = { beforeMs: 9007199254740991n, beforeId: "" };
// The agent memory fixture: an agent, a peer in its family, and an agent of
// another family. The service actor writes all three rows.
const SERVICE = `service-${RUN}`;
const AGENT = `agent-${RUN}`;
const PEER = `peer-${RUN}`;
const STRANGER = `stranger-${RUN}`;
const FAMILY = `family-${RUN}`;
const OTHER_FAMILY = `other-family-${RUN}`;
// The graph fixture: two graphs, and receipts of two clients of one actor.
// Client scopes follow the contract's `<a>:<actor><c>:<client>` byte lengths.
const GRAPH = `graph-${RUN}`;
const OTHER_GRAPH = `other-graph-${RUN}`;
const clientScope = (actor: string, client: string) =>
  `${Buffer.byteLength(actor)}:${actor}${Buffer.byteLength(client)}:${client}`;
const OWN_RECEIPTS = clientScope(ACTOR, CLIENT);
const OTHER_RECEIPTS = clientScope(ACTOR, `${CLIENT}-2`);
const CONVERSATION_SCOPES = {
  dir: [PARTITION],
  session: [SESSION],
  member: [ACTOR],
  page: [PAGE],
  trace: [PAGE],
  actor: [ACTOR],
  agent: [AGENT],
  family: [FAMILY],
};

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
    resolveScopes: ({ actorId }) =>
      actorId === SERVICE
        ? {
            agent: [AGENT, PEER, STRANGER],
            family: [FAMILY, OTHER_FAMILY],
            kg_graph: [GRAPH, OTHER_GRAPH],
            kg_client: [OWN_RECEIPTS, OTHER_RECEIPTS],
          }
        : { session: [SESSION], member: [ACTOR], page: [PAGE], trace: [PAGE], actor: [ACTOR] },
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

/** The rows of `table` a subscription receives, through the server's own matching. */
async function rowsFor(
  server: SyncServerConfig,
  table: string,
  requested: ScopeMap,
  allowed: ScopeMap,
): Promise<string[]> {
  const outcome = computeEffective(requested, { ok: true, allowed });
  if (outcome.status !== "active") return [];
  const rows = await server.storage.scanRows(PARTITION, {
    table,
    scopeFilter: outcome.effective,
    afterRowId: null,
    limit: 100,
  });
  return rows.map((row) => row.rowId).sort();
}

/** One graph's worth of every graph table, written in one commit. */
function graphRows(graph: string, now: number) {
  const id = (table: string) => `${table}-${graph}`;
  const stamp = { graph, author_actor: SERVICE, author_client: SERVICE };
  const event = id("event");
  const made = { created_event_id: event, updated_event_id: event };
  const rows: Record<string, Record<string, unknown>> = {
    kg_events: {
      graph_seq: "1",
      descriptor: "graph.apply",
      request_id: id("request"),
      request_digest: "0".repeat(64),
      actor_id: ACTOR,
      client_id: CLIENT,
      origin_class: "human",
      recorded_at_ms: now,
      op_count: 1,
      delta_digest: "0".repeat(64),
    },
    kg_event_ops: {
      event_id: event,
      ordinal: 0,
      entity_type: "node",
      entity_id: id("node"),
      operation: "create",
      after_revision: "1",
      after_value: "{}",
    },
    kg_head: { graph_seq: "1", event_id: event },
    kg_nodes: {
      kind: "Concept",
      title: "live check",
      properties: "{}",
      lifecycle: "active",
      revision: "1",
      lifecycle_revision: "1",
      ...made,
    },
    kg_edges: {
      from_id: id("node"),
      to_id: id("node"),
      type: "sources",
      properties: "{}",
      lifecycle: "active",
      revision: "1",
      ...made,
    },
    kg_tags: { text: "live-check", revision: "1", ...made },
    kg_node_tags: { node_id: id("node"), tag_id: id("tag"), created_event_id: event },
  };
  const ids: Record<string, string> = {
    kg_events: event,
    kg_event_ops: `${event}/0`,
    kg_head: graph,
    kg_nodes: id("node"),
    kg_edges: id("edge"),
    kg_tags: id("tag"),
    kg_node_tags: id("node_tag"),
  };
  return Object.entries(rows).map(([table, values]) => ({
    table,
    op: "upsert" as const,
    values: { id: ids[table], ...values, ...stamp },
  }));
}

function receipt(scope: string, now: number) {
  return {
    table: "kg_receipts",
    op: "upsert" as const,
    values: {
      id: `graph.apply/request-${RUN}/${scope}`,
      client_scope: scope,
      descriptor: "graph.apply",
      request_id: `request-${RUN}`,
      request_digest: "0".repeat(64),
      outcome: "rejected",
      reason: "invalid_change",
      detail: "{}",
      recorded_at_ms: now,
      author_actor: SERVICE,
      author_client: SERVICE,
    },
  };
}

async function checkOne(url: string): Promise<void> {
  const sql = new SQL(url);
  const [{ schema_name: schemaName, role }] = await sql`
    SELECT current_schema() AS schema_name, current_user AS role`;
  if (schemaName === null || schemaName === "public") {
    fail(`${role}: search_path must name the role's own schema, got ${schemaName}`);
  }
  const [{ count: before }] = await sql`
    SELECT count(*)::int AS count FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
     WHERE n.nspname = ${schemaName}`;
  if (before !== 0) fail(`${role}: ${schemaName} holds ${before} relations; use a fresh schema`);
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
  const messageIds = [0, 1, 2].map((n) => `message-${RUN}-${n}`);
  await seedMutations(
    first,
    { partition: PARTITION, actorId: ACTOR, clientId: CLIENT, commitId: `messages-${RUN}` },
    [
      {
        table: "sessions",
        op: "upsert",
        values: {
          id: SESSION,
          title: "live check",
          status: "open",
          agent_actor: AGENT,
          created_by_actor: ACTOR,
          created_by_client: CLIENT,
          invite: "[]",
          archived: true,
          created_at_ms: now,
          last_activity_ms: now + 2,
          ...authored,
        },
      },
      ...messageIds.map((id, n) => ({
        table: "messages",
        op: "upsert" as const,
        values: {
          id,
          session_id: SESSION,
          page_key: PAGE,
          role: "person",
          body: `message ${n}`,
          created_at_ms: now + n,
          ...authored,
        },
      })),
    ],
  );
  const memory = (id: string, agent: string, family: string) => ({
    table: "agent_memory",
    op: "upsert" as const,
    values: {
      id,
      agent_actor: agent,
      family,
      kind: "journal",
      text: `${agent} wrote this`,
      created_at_ms: now,
      author_actor: SERVICE,
      author_client: SERVICE,
    },
  });
  const ownRow = `memory-${RUN}-own`;
  const peerRow = `memory-${RUN}-peer`;
  const strangerRow = `memory-${RUN}-stranger`;
  await seedMutations(
    first,
    { partition: PARTITION, actorId: SERVICE, clientId: SERVICE, commitId: `memory-${RUN}` },
    [
      memory(ownRow, AGENT, FAMILY),
      memory(peerRow, PEER, FAMILY),
      memory(strangerRow, STRANGER, OTHER_FAMILY),
    ],
  );
  const [{ count: commits }] = await sql`
    SELECT count(*)::int AS count FROM sync_commits WHERE partition = ${PARTITION}`;
  if (commits !== 2) fail(`${role}: ${commits} commits in this run's partition, expected 2`);

  const registry = new RemoteOperationRegistry([
    registerRemoteQuery(sessionPageQuery, { maxRows: 10_000, auth: { access: "scoped" } }),
  ]);
  const page1 = await runSessionPage(first, registry, {
    pageKey: PAGE,
    ...FIRST_PAGE_CURSOR,
    pageSize: 2n,
  });
  if (page1.kind !== "query") fail(`${role}: sessionPage failed: ${JSON.stringify(page1)}`);
  const ids1 = page1.rows.map((row) => row.id);
  if (JSON.stringify(ids1) !== JSON.stringify([messageIds[2], messageIds[1]])) {
    fail(`${role}: first page ${JSON.stringify(ids1)}`);
  }
  const last = page1.rows[1] as { created_at_ms: number; id: string };
  const page2 = await runSessionPage(first, registry, {
    pageKey: PAGE,
    beforeMs: BigInt(last.created_at_ms),
    beforeId: last.id,
    pageSize: 2n,
  });
  if (page2.kind !== "query") fail(`${role}: sessionPage failed: ${JSON.stringify(page2)}`);
  const ids2 = page2.rows.map((row) => row.id);
  if (JSON.stringify(ids2) !== JSON.stringify([messageIds[0]])) {
    fail(`${role}: second page ${JSON.stringify(ids2)}`);
  }
  const outside = await runSessionPage(first, registry, {
    pageKey: OTHER_PAGE,
    ...FIRST_PAGE_CURSOR,
    pageSize: 2n,
  });
  if (outside.kind !== "error" || outside.code !== "operation.forbidden") {
    fail(`${role}: a page outside the actor's scopes answered ${JSON.stringify(outside)}`);
  }

  // What resolveScopes grants an agent instance: itself and its own family.
  const instance = { agent: [AGENT], family: [FAMILY] };
  const own = await rowsFor(
    first,
    "agent_memory",
    agentMemoryOwnSubscription.scopes({ agentActor: AGENT }),
    instance,
  );
  const family = await rowsFor(
    first,
    "agent_memory",
    agentMemoryFamilySubscription.scopes({ family: FAMILY }),
    instance,
  );
  const foreign = await rowsFor(
    first,
    "agent_memory",
    agentMemoryFamilySubscription.scopes({ family: OTHER_FAMILY }),
    instance,
  );
  if (JSON.stringify(own) !== JSON.stringify([ownRow])) {
    fail(`${role}: own memory subscription received ${JSON.stringify(own)}`);
  }
  if (JSON.stringify(family) !== JSON.stringify([ownRow, peerRow].sort())) {
    fail(`${role}: family memory subscription received ${JSON.stringify(family)}`);
  }
  if (foreign.length !== 0) {
    fail(`${role}: another family's memory reached the agent: ${JSON.stringify(foreign)}`);
  }

  // The graph: two graphs and two clients' receipts, then each subscription
  // template under what a graph reader, a reader of the other graph, a
  // conversation-only caller and each client hold.
  await seedMutations(
    first,
    { partition: PARTITION, actorId: SERVICE, clientId: SERVICE, commitId: `graph-${RUN}` },
    graphRows(GRAPH, now),
  );
  await seedMutations(
    first,
    { partition: PARTITION, actorId: SERVICE, clientId: SERVICE, commitId: `other-graph-${RUN}` },
    graphRows(OTHER_GRAPH, now),
  );
  await seedMutations(
    first,
    { partition: PARTITION, actorId: SERVICE, clientId: SERVICE, commitId: `receipts-${RUN}` },
    [receipt(OWN_RECEIPTS, now), receipt(OTHER_RECEIPTS, now)],
  );
  const graphSubscriptions = [
    kgEventsSubscription,
    kgEventOpsSubscription,
    kgHeadSubscription,
    kgNodesSubscription,
    kgEdgesSubscription,
    kgTagsSubscription,
    kgNodeTagsSubscription,
  ];
  for (const subscription of graphSubscriptions) {
    const { table } = subscription;
    const reader = await rowsFor(first, table, subscription.scopes({ database: GRAPH }), {
      kg_graph: [GRAPH],
    });
    const otherReader = await rowsFor(first, table, subscription.scopes({ database: GRAPH }), {
      kg_graph: [OTHER_GRAPH],
    });
    const conversationOnly = await rowsFor(
      first,
      table,
      subscription.scopes({ database: GRAPH }),
      CONVERSATION_SCOPES,
    );
    if (reader.length !== 1 || !reader[0]?.includes(GRAPH) || reader[0]?.includes(OTHER_GRAPH)) {
      fail(`${role}: ${table} for its graph's reader received ${JSON.stringify(reader)}`);
    }
    if (otherReader.length !== 0 || conversationOnly.length !== 0) {
      fail(
        `${role}: ${table} reached a caller without its graph: ` +
          `${JSON.stringify({ otherReader, conversationOnly })}`,
      );
    }
  }
  const ownReceipts = await rowsFor(
    first,
    "kg_receipts",
    kgReceiptsSubscription.scopes({ clientScope: OWN_RECEIPTS }),
    { kg_client: [OWN_RECEIPTS], kg_graph: [GRAPH, OTHER_GRAPH] },
  );
  const otherClients = await rowsFor(
    first,
    "kg_receipts",
    kgReceiptsSubscription.scopes({ clientScope: OTHER_RECEIPTS }),
    { kg_client: [OWN_RECEIPTS], kg_graph: [GRAPH, OTHER_GRAPH] },
  );
  if (
    JSON.stringify(ownReceipts) !== JSON.stringify([`graph.apply/request-${RUN}/${OWN_RECEIPTS}`])
  ) {
    fail(`${role}: own receipts received ${JSON.stringify(ownReceipts)}`);
  }
  if (otherClients.length !== 0) {
    fail(`${role}: another client's receipt reached this one: ${JSON.stringify(otherClients)}`);
  }

  await sql.end();
  console.log(
    `${role} → ${schemaName}: empty before, ${contract.length} tables there and none in public, ` +
      `second start a no-op, this run's commits ${commits}, sessionPage paged ` +
      `${ids1.length}+${ids2.length} fresh rows, foreign page refused, memory: own ${own.length}, ` +
      `family ${family.length} (with the peer), other family ${foreign.length}, graph: ` +
      `${graphSubscriptions.length} tables one graph each, none without kg_graph, ` +
      `receipts own ${ownReceipts.length} other client ${otherClients.length}`,
  );
}

const urls = (process.env.KEEPER_SYNC_TEST_PG_URL ?? "").split(/\s+/).filter(Boolean);
if (urls.length === 0) fail("KEEPER_SYNC_TEST_PG_URL is not set");
for (const url of urls) await checkOne(url);
