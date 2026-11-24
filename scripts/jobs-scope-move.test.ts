import {
  decodeRemoteOperationResponse,
  encodeRemoteOperationRequest,
  type RowValue,
  type ScopeMap,
} from "@syncular/core";
import {
  type CommandMutation,
  compileSchema,
  computeEffective,
  coveredTables,
  ensureSyncServerReady,
  handleRemoteOperation,
  MemorySegmentStore,
  RemoteOperationRegistry,
  registerRemoteCommand,
  remoteCommand,
  type ServerSchema,
  type StoredRow,
  type SyncServerConfig,
  seedMutations,
} from "@syncular/server";
import {
  NodeSqliteDatabase,
  type SqliteDatabase,
  SqliteServerStorage,
  type SqliteStatement,
  type SqliteValue,
} from "@syncular/server/sqlite";
import { afterEach, describe, expect, it } from "vitest";
import { schema as generatedSchema, jobsPageSubscription } from "../syncular/syncular.generated";

const schema = generatedSchema as unknown as ServerSchema;
const compiled = compileSchema(schema);

const PARTITION = "jobs-scope-move-test";
const DOMAIN = "d";
const SERVICE = "service-jobs";
const SUBMITTER_ACTOR = "actor-1";
const SUBMITTER_CLIENT = "client-1";
const WORKER_ACTOR = "worker-1";
const WORKER_CLIENT = "worker-client-1";
const JOB_ID = "01J00000000000000000000001";
const ATTEMPT_ID = "01J00000000000000000000002";
const SUBMITTED_AT_MS = 1792454400000; // 2026-10-20T00:00:00Z
const ENDED_AT_MS = 1795996800000; // 2026-11-30T00:00:00Z (UTC month 2026-11)

const ALLOWED_SCOPES: ScopeMap = {
  jobs_page: [`${DOMAIN}/open`, `${DOMAIN}/2026-10`, `${DOMAIN}/2026-11`],
  jobs_nodes: [DOMAIN],
};

async function scanUnitRows(
  server: SyncServerConfig,
  table: string,
  unit: Record<string, string>,
  allowed: ScopeMap,
): Promise<StoredRow[]> {
  const covered = coveredTables(compiled, unit).find((candidate) => candidate.name === table);
  if (covered === undefined) return [];
  const requested: ScopeMap = Object.fromEntries(
    Object.entries(unit).map(([variable, value]) => [variable, [value]]),
  );
  const outcome = computeEffective(covered.declaredVariables, requested, { ok: true, allowed });
  if (outcome.status !== "active") return [];
  return server.storage.scanRows(PARTITION, {
    table,
    scopeFilter: outcome.effective,
    afterRowId: null,
    limit: 100,
  });
}

function makeJobRow(overrides: Partial<Record<string, RowValue>> = {}): Record<string, RowValue> {
  return {
    id: JOB_ID,
    domain: DOMAIN,
    page_key: `${DOMAIN}/open`,
    kind: "test.alpha",
    input: JSON.stringify({ entry_id: "entry-1", content_id: "content-1" }),
    input_blob: null,
    input_digest: "0".repeat(64),
    runs_on: "server",
    state: "claimed",
    attempt: 1,
    max_attempts: 3,
    not_before_ms: SUBMITTED_AT_MS,
    plan_blob: null,
    plan_attempt_id: null,
    note: null,
    result: null,
    result_blob: null,
    effects: null,
    error_code: null,
    error_sentence: null,
    submitter_actor: SUBMITTER_ACTOR,
    submitter_client: SUBMITTER_CLIENT,
    submitter_context: null,
    submitted_at_ms: SUBMITTED_AT_MS,
    updated_at_ms: SUBMITTED_AT_MS,
    ended_at_ms: null,
    revision: 2,
    author_actor: SERVICE,
    author_client: SERVICE,
    ...overrides,
  };
}

function makeClaimRow(overrides: Partial<Record<string, RowValue>> = {}): Record<string, RowValue> {
  return {
    job_id: JOB_ID,
    domain: DOMAIN,
    page_key: `${DOMAIN}/open`,
    holder_actor: WORKER_ACTOR,
    holder_client: WORKER_CLIENT,
    attempt_id: ATTEMPT_ID,
    epoch: 1,
    lease_ms: 60_000,
    acquired_at_ms: SUBMITTED_AT_MS,
    renewed_at_ms: SUBMITTED_AT_MS,
    released: false,
    revision: 1,
    author_actor: SERVICE,
    author_client: SERVICE,
    ...overrides,
  };
}

async function runCommand(
  server: SyncServerConfig,
  operationId: string,
  requestId: string,
  mutations: readonly CommandMutation[],
) {
  const descriptor = remoteCommand<Record<string, never>>(operationId);
  const registry = new RemoteOperationRegistry([
    registerRemoteCommand(descriptor, {
      authorize: () => true,
      run: () => mutations,
    }),
  ]);
  const bytes = await handleRemoteOperation(
    encodeRemoteOperationRequest({
      revision: 1,
      kind: "command",
      clientId: "worker-host",
      operationId,
      requestId,
      params: {},
    }),
    { ...server, partition: PARTITION, actorId: SERVICE },
    registry,
  );
  return decodeRemoteOperationResponse(bytes);
}

/** The test environment is jsdom, whose `Uint8Array` is not Node's: node:sqlite's
 * BLOB values fail the storage's `instanceof Uint8Array`. Copy them into this
 * realm's arrays on the way out; nothing else changes. */
function sameRealm(value: SqliteValue): SqliteValue {
  return typeof value === "object" && value !== null && ArrayBuffer.isView(value)
    ? new Uint8Array(value as Uint8Array)
    : value;
}

function sameRealmRow<Row>(row: Row): Row {
  if (row === null || typeof row !== "object") return row;
  return Object.fromEntries(
    Object.entries(row as Record<string, SqliteValue>).map(([k, v]) => [k, sameRealm(v)]),
  ) as Row;
}

function inThisRealm(db: SqliteDatabase): SqliteDatabase {
  return {
    exec: (sql) => db.exec(sql),
    run: (sql, bindings) => db.run(sql, bindings),
    query<Row = Record<string, SqliteValue>, Params extends readonly SqliteValue[] = SqliteValue[]>(
      sql: string,
    ): SqliteStatement<Row, Params> {
      const statement = db.query<Row, Params>(sql);
      return {
        run: (...params) => statement.run(...params),
        get: (...params) => {
          const row = statement.get(...params);
          return row === null ? null : sameRealmRow(row);
        },
        all: (...params) => statement.all(...params).map(sameRealmRow),
      };
    },
    close: () => db.close(),
  };
}

describe("jobs explicit scope move (rescope)", () => {
  let db: NodeSqliteDatabase | undefined;

  afterEach(() => {
    db?.close();
    db = undefined;
  });

  it("moves jobs and job_claims from <domain>/open to <domain>/<yyyy-mm> on completion and rejects non-scope changes in rescope", async () => {
    db = new NodeSqliteDatabase(":memory:");
    const server: SyncServerConfig = {
      schema,
      storage: new SqliteServerStorage(inThisRealm(db)),
      segments: new MemorySegmentStore(),
      resolveScopes: ({ actorId }) => (actorId === SERVICE ? ALLOWED_SCOPES : {}),
    };
    await ensureSyncServerReady(server);

    await seedMutations(
      server,
      {
        partition: PARTITION,
        actorId: SERVICE,
        clientId: "seed-client",
        commitId: "seed-open-job",
      },
      [
        { table: "jobs", op: "upsert", values: makeJobRow() },
        { table: "job_claims", op: "upsert", values: makeClaimRow() },
      ],
    );

    const openUnit = jobsPageSubscription.scopes({ pageKey: `${DOMAIN}/open` });
    const octUnit = jobsPageSubscription.scopes({ pageKey: `${DOMAIN}/2026-10` });
    const novUnit = jobsPageSubscription.scopes({ pageKey: `${DOMAIN}/2026-11` });

    const openJobsBefore = await scanUnitRows(server, "jobs", openUnit, ALLOWED_SCOPES);
    const openClaimsBefore = await scanUnitRows(server, "job_claims", openUnit, ALLOWED_SCOPES);
    expect(openJobsBefore.map((r) => r.rowId)).toEqual([JOB_ID]);
    expect(openClaimsBefore.map((r) => r.rowId)).toEqual([JOB_ID]);
    expect(await scanUnitRows(server, "jobs", novUnit, ALLOWED_SCOPES)).toEqual([]);
    expect(await scanUnitRows(server, "job_claims", novUnit, ALLOWED_SCOPES)).toEqual([]);
    expect(await scanUnitRows(server, "jobs", octUnit, ALLOWED_SCOPES)).toEqual([]);
    expect(await scanUnitRows(server, "job_claims", octUnit, ALLOWED_SCOPES)).toEqual([]);

    const jobVersionBefore = openJobsBefore[0].serverVersion;
    const claimVersionBefore = openClaimsBefore[0].serverVersion;

    const completedJobOpen = makeJobRow({
      state: "succeeded",
      result: JSON.stringify({ ok: true }),
      effects: JSON.stringify([]),
      updated_at_ms: ENDED_AT_MS,
      ended_at_ms: ENDED_AT_MS,
      revision: 3,
    });
    const completedJobMoved = { ...completedJobOpen, page_key: `${DOMAIN}/2026-11` };

    const releasedClaimOpen = makeClaimRow({
      released: true,
      revision: 2,
    });
    const releasedClaimMoved = { ...releasedClaimOpen, page_key: `${DOMAIN}/2026-11` };

    const completeRes = await runCommand(server, "jobs.complete", "req-complete-1", [
      { table: "jobs", op: "upsert", values: completedJobOpen },
      { table: "jobs", op: "rescope", values: completedJobMoved },
      { table: "job_claims", op: "upsert", values: releasedClaimOpen },
      { table: "job_claims", op: "rescope", values: releasedClaimMoved },
    ]);
    expect(completeRes.kind).toBe("command");
    if (completeRes.kind !== "command") throw new Error("expected command response");
    expect(completeRes.status).toBe("applied");

    expect(await scanUnitRows(server, "jobs", openUnit, ALLOWED_SCOPES)).toEqual([]);
    expect(await scanUnitRows(server, "job_claims", openUnit, ALLOWED_SCOPES)).toEqual([]);
    expect(await scanUnitRows(server, "jobs", octUnit, ALLOWED_SCOPES)).toEqual([]);
    expect(await scanUnitRows(server, "job_claims", octUnit, ALLOWED_SCOPES)).toEqual([]);

    const novJobsAfter = await scanUnitRows(server, "jobs", novUnit, ALLOWED_SCOPES);
    const novClaimsAfter = await scanUnitRows(server, "job_claims", novUnit, ALLOWED_SCOPES);
    expect(novJobsAfter.map((r) => r.rowId)).toEqual([JOB_ID]);
    expect(novClaimsAfter.map((r) => r.rowId)).toEqual([JOB_ID]);
    expect(novJobsAfter[0].serverVersion).toBeGreaterThan(jobVersionBefore);
    expect(novClaimsAfter[0].serverVersion).toBeGreaterThan(claimVersionBefore);

    const smuggledRes = await runCommand(server, "jobs.smuggle", "req-smuggle-1", [
      {
        table: "jobs",
        op: "rescope",
        values: {
          ...completedJobMoved,
          page_key: `${DOMAIN}/2026-10`,
          state: "failed",
        },
      },
    ]);
    expect(smuggledRes.kind).toBe("command");
    if (smuggledRes.kind !== "command") throw new Error("expected command response");
    expect(smuggledRes.status).toBe("rejected");
    const failedOp = smuggledRes.results.find((r) => r.status !== "applied");
    expect(failedOp?.code).toBe("sync.invalid_request");
    expect(failedOp?.message).toContain("rescope may change only scope columns");
  });
});
