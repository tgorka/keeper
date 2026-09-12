import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import {
  Api,
  configuration,
  loadManifest,
  reconcile,
  resources,
  run,
  safeMessage,
  smoke,
  validateManifest,
} from "./provision.mjs";

const personal = `phx_${"synthetic".repeat(8)}`;
const projectToken = `phc_${"fixture".repeat(8)}`;
const env = {
  POSTHOG_HOST: "https://us.posthog.com",
  POSTHOG_PROJECT_ID: "605256",
  POSTHOG_INGEST_HOST: "https://us.i.posthog.com",
  POSTHOG_PERSONAL_API_KEY: personal,
  POSTHOG_PROJECT_API_KEY: projectToken,
};
// A fixture writer must never acquire or remove the real maintainer apply lock.
const previousTmpdir = process.env.TMPDIR;
const testTmpdir = await mkdtemp(join(tmpdir(), "keeper-posthog-tests-"));
process.env.TMPDIR = testTmpdir;
after(async () => {
  if (previousTmpdir === undefined) delete process.env.TMPDIR;
  else process.env.TMPDIR = previousTmpdir;
  await rm(testTmpdir, { recursive: true, force: true });
});
const json = (body, status = 200) => new Response(JSON.stringify(body), { status });

function server(manifest) {
  const calls = [];
  const stores = new Map();
  const project = {
    id: 605256,
    name: "keeper",
    api_token: projectToken,
    marketing_analytics_config: { conversion_goals: [] },
  };
  let counter = 0;
  let captured;
  for (const resource of resources(manifest)) stores.set(resource.collection, []);
  const fetcher = async (input, options) => {
    const url = new URL(input);
    const body = options.body === undefined ? undefined : JSON.parse(options.body);
    calls.push({
      url,
      method: options.method,
      headers: options.headers,
      body,
      redirect: options.redirect,
    });
    if (url.origin === "https://us.i.posthog.com") {
      if (url.pathname === "/flags/")
        return json({
          featureFlagPayloads: {
            "keeper-client-config": JSON.stringify({
              supportMessage: manifest.remoteConfig.supportMessage,
            }),
          },
        });
      captured = body;
      return json({ status: 1 });
    }
    const path = url.pathname.replace("/api/projects/605256/", "");
    if (!path) return json(project);
    if (path === "marketing_analytics/conversion_goals/") return json({ goals: [] });
    if (path === "marketing_analytics/conversion_goals/create/") {
      const goal = { ...body.goal, conversion_goal_id: String(++counter) };
      project.marketing_analytics_config.conversion_goals.push(goal);
      return json({ goal });
    }
    if (path === "query/") return json({ results: [[captured ? 1 : 0]] });
    if (path.startsWith("endpoints/") && path.endsWith("/run/")) return json({ results: [[0]] });
    const collection = [...stores.keys()].find((key) => path.startsWith(`${key}/`));
    assert.ok(collection, "fixture must explicitly implement each API route");
    const rows = stores.get(collection);
    const key = path.slice(collection.length + 1).replace(/\/$/, "");
    if (options.method === "GET") {
      if (!key) return json({ results: rows, next: null });
      const row = rows.find((r) => String(r.id) === key || r.name === key);
      return row ? json(row) : json({}, 404);
    }
    const row =
      options.method === "POST"
        ? { id: ++counter, status: "proposed", is_drifted: false }
        : rows.find((r) => String(r.id) === key || r.name === key);
    assert.ok(row);
    Object.assign(row, body);
    // Real PostHog normalizes tags and returns them in no declared order.
    if (Array.isArray(row.tags)) row.tags = row.tags.map((tag) => tag.toLowerCase()).reverse();
    if (body.dashboards) {
      row.dashboard_tiles = body.dashboards.map((dashboard_id) => ({
        dashboard_id,
        id: ++counter,
      }));
      delete row.dashboards;
    }
    if (options.method === "POST") rows.push(row);
    return json(row);
  };
  return { calls, stores, project, fetcher };
}

const writes = (s) => s.calls.filter((c) => c.method !== "GET");

test("default dry-run needs no key, makes zero requests, and never prints env values", async () => {
  const output = [];
  await run(
    [],
    { POSTHOG_HOST: env.POSTHOG_HOST, POSTHOG_PROJECT_ID: env.POSTHOG_PROJECT_ID },
    (s) => output.push(s),
    () => assert.fail("dry run contacted network"),
  );
  assert.match(output.join("\n"), /Dry run: no network/);
  assert.ok(!output.join("\n").includes(personal));
});

test("apply requires reviewed digest and correct explicit host/project before network", async () => {
  const noNetwork = () => assert.fail("invalid invocation contacted network");
  await assert.rejects(
    run(["--apply"], env, () => {}, noNetwork),
    /reviewed-sha256/,
  );
  await assert.rejects(
    run(["--apply", `--reviewed-sha256=${"0".repeat(64)}`], env, () => {}, noNetwork),
    /reviewed-sha256/,
  );
  await assert.rejects(
    run(["--plan"], { ...env, POSTHOG_HOST: "https://attacker.invalid" }, () => {}, noNetwork),
    /reviewed Keeper target/,
  );
  await assert.rejects(
    run(["--plan"], { ...env, POSTHOG_PROJECT_ID: "1" }, () => {}, noNetwork),
    /reviewed Keeper target/,
  );
  await assert.rejects(
    run(["--smoke"], env, () => {}, noNetwork),
    /requires --smoke --apply/,
  );
});

test("trusted CI rejects forks, PR events and non-main branches before using key", async () => {
  const { manifest } = await loadManifest();
  for (const overrides of [
    { GITHUB_EVENT_NAME: "pull_request_target" },
    { GITHUB_REPOSITORY: "other/keeper" },
    { GITHUB_REF: "refs/heads/feature" },
  ]) {
    assert.throws(
      () =>
        configuration(
          {
            ...env,
            CI: "true",
            GITHUB_EVENT_NAME: "workflow_dispatch",
            GITHUB_REF: "refs/heads/main",
            GITHUB_REPOSITORY: "tgorka/keeper",
            ...overrides,
          },
          manifest,
        ),
      /trusted manual main/,
    );
  }
});

test("personas reject unknown events and diagnostics events before provisioning", async () => {
  const { manifest } = await loadManifest();
  for (const event of ["keeper_settings_typo", "keeper_app_ready"]) {
    const changed = structuredClone(manifest);
    changed.personas[0].event = event;
    assert.throws(() => validateManifest(changed), /productAnalytics catalog/);
  }
});

test("plan is GET-only; repeated apply has no duplicate resources or write churn", async () => {
  const { manifest, digest } = await loadManifest();
  const s = server(manifest);
  await run(["--plan"], env, () => {}, s.fetcher);
  assert.equal(writes(s).length, 0);
  await run(["--apply", `--reviewed-sha256=${digest}`], env, () => {}, s.fetcher);
  const count = writes(s).length;
  assert.ok(count > 0);
  await run(["--apply", `--reviewed-sha256=${digest}`], env, () => {}, s.fetcher);
  assert.equal(writes(s).length, count);
  await run(["--verify"], env, () => {}, s.fetcher);
  assert.equal(writes(s).length, count);
  assert.equal(s.project.marketing_analytics_config.conversion_goals.length, 1);
  for (const rows of s.stores.values())
    assert.equal(new Set(rows.map((r) => r.name ?? r.key)).size, rows.length);
});

test("owned configuration drift is repaired but approved metric drift blocks all writes", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  const api = new Api(configuration(env, manifest), s.fetcher);
  await reconcile(api, manifest, "apply", () => {});
  s.stores.get("feature_flags")[0].active = false;
  await assert.rejects(
    reconcile(api, manifest, "verify", () => {}),
    /differ/,
  );
  await reconcile(api, manifest, "apply", () => {});
  assert.equal(s.stores.get("feature_flags")[0].active, true);
  const metric = s.stores.get("data_catalog/metrics")[0];
  metric.status = "approved";
  await reconcile(api, manifest, "apply", () => {});
  metric.definition.query = "SELECT 1";
  const count = writes(s).length;
  await assert.rejects(
    reconcile(api, manifest, "apply", () => {}),
    /Approved metric drift/,
  );
  assert.equal(writes(s).length, count);
});

test("reconcile clears uncalibrated confidence on an existing proposed metric", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  const api = new Api(configuration(env, manifest), s.fetcher);
  await reconcile(api, manifest, "apply", () => {});
  const metric = s.stores.get("data_catalog/metrics")[0];
  metric.confidence = 0.8;
  await assert.rejects(
    reconcile(api, manifest, "verify", () => {}),
    /differ/,
  );
  await reconcile(api, manifest, "apply", () => {});
  assert.equal(metric.confidence, null);
  assert.equal(metric.status, "proposed");
});

test("name collisions and unavailable capabilities abort before any mutation", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  s.stores
    .get("dashboards")
    .push({ id: 321, name: manifest.dashboard.name, description: "Human's unrelated dashboard" });
  await assert.rejects(
    reconcile(new Api(configuration(env, manifest), s.fetcher), manifest, "apply", () => {}),
    /collides/,
  );
  assert.equal(writes(s).length, 0);
  s.stores.get("dashboards").length = 0;
  const unavailable = async (url, options) =>
    new URL(url).pathname.endsWith("/data_catalog/metrics/")
      ? json({ detail: personal }, 404)
      : s.fetcher(url, options);
  await assert.rejects(
    reconcile(new Api(configuration(env, manifest), unavailable), manifest, "apply", () => {}),
    /HTTP 404; response suppressed/,
  );
  assert.equal(writes(s).length, 0);
});

test("duplicate resources on later pages are not silently adopted or recreated", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  const resource = resources(manifest)[0];
  const paginated = async (input, options) => {
    const url = new URL(input);
    if (url.pathname.endsWith("/event_definitions/"))
      return json({
        results: [{ ...resource.body, id: url.searchParams.has("offset") ? 2 : 1 }],
        next: url.searchParams.has("offset") ? null : `${url.origin}${url.pathname}?offset=1`,
      });
    return s.fetcher(input, options);
  };
  await assert.rejects(
    reconcile(new Api(configuration(env, manifest), paginated), manifest, "apply", () => {}),
    /Duplicate managed/,
  );
  assert.equal(writes(s).length, 0);
});

test("pagination cannot leak credentials to a different host, project or resource", async () => {
  const { manifest } = await loadManifest();
  for (const next of [
    "https://attacker.invalid/api/projects/605256/endpoints/",
    "https://us.posthog.com/api/projects/999/endpoints/",
    "https://us.posthog.com/api/projects/605256/persons/",
  ]) {
    let count = 0;
    const api = new Api(configuration(env, manifest), async () => {
      count++;
      return json({ results: [], next });
    });
    await assert.rejects(api.list("endpoints"), /pagination URL/);
    assert.equal(count, 1);
  }
});

test("transport errors, response bodies and oversize responses cannot expose keys", async () => {
  const { manifest } = await loadManifest();
  for (const response of [
    async () => {
      throw new Error(`request ${personal}`);
    },
    async () => json({ detail: personal }, 403),
    async () => new Response(personal),
    async () => new Response("x".repeat(2 * 1024 * 1024 + 1)),
  ]) {
    const api = new Api(configuration(env, manifest), response);
    try {
      await api.request(api.root);
      assert.fail("unsafe response accepted");
    } catch (error) {
      assert.ok(!safeMessage(error).includes(personal));
      assert.ok(!safeMessage(error).includes("synthetic"));
    }
  }
});

test("public requests never inherit personal auth and synthetic smoke never creates profiles", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  const output = [];
  await smoke(
    new Api(configuration(env, manifest), s.fetcher),
    manifest,
    env,
    (v) => output.push(v),
    async () => {},
  );
  const publicCalls = s.calls.filter((c) => c.url.origin === manifest.target.ingestHost);
  assert.equal(publicCalls.length, 2);
  for (const call of publicCalls) {
    assert.equal(call.headers.Authorization, undefined);
    assert.equal(call.redirect, "error");
    assert.ok(!JSON.stringify(call.body).includes(personal));
  }
  const event = publicCalls.find((c) => c.body.event)?.body;
  assert.equal(event.event, "keeper_ops_smoke");
  assert.equal(event.properties.synthetic, true);
  assert.equal(event.properties.$process_person_profile, false);
  assert.equal(event.properties.$geoip_disable, true);
  assert.deepEqual(Object.keys(event.properties).sort(), [
    "$geoip_disable",
    "$process_person_profile",
    "distinct_id",
    "event_source",
    "synthetic",
  ]);
  assert.ok(output.includes("verified synthetic_ingestion keeper_ops_smoke"));
  assert.ok(!output.join("\n").includes(projectToken));
  assert.equal(
    s.calls.filter((c) => c.url.pathname.endsWith("/run/")).length,
    manifest.metrics.length,
  );
});

test("synthetic smoke refuses a project token mismatch before ingestion", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  s.project.api_token = "phc_different";
  await assert.rejects(
    smoke(new Api(configuration(env, manifest), s.fetcher), manifest, env),
    /token mismatch/,
  );
  assert.equal(writes(s).length, 0);
});

test("smoke observation deadline consumes an attempt without resending ingestion", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  let observations = 0;
  const fetcher = (url, options) => {
    if (new URL(url).pathname.endsWith("/query/") && ++observations === 1) {
      throw new DOMException("synthetic observation deadline", "TimeoutError");
    }
    return s.fetcher(url, options);
  };
  await smoke(
    new Api(configuration(env, manifest), fetcher),
    manifest,
    env,
    () => {},
    async () => {},
  );
  assert.equal(observations, 2);
  assert.equal(s.calls.filter((call) => call.url.pathname === "/i/v0/e/").length, 1);
});

test("ambiguous write failure is not retried; next apply discovers the successful write", async () => {
  const { manifest } = await loadManifest();
  const s = server(manifest);
  let interrupted = false;
  const lostResponse = async (url, options) => {
    const response = await s.fetcher(url, options);
    if (!interrupted && options.method === "POST") {
      interrupted = true;
      throw new Error(`lost response ${personal}`);
    }
    return response;
  };
  await assert.rejects(
    reconcile(new Api(configuration(env, manifest), lostResponse), manifest, "apply", () => {}),
    /No automatic write retry/,
  );
  assert.equal(writes(s).length, 1);
  await reconcile(new Api(configuration(env, manifest), s.fetcher), manifest, "apply", () => {});
  assert.equal(s.stores.get("event_definitions").length, manifest.events.length);
});

test("concurrent local applies cannot race list then create", async () => {
  const { manifest, digest } = await loadManifest();
  const s = server(manifest);
  let unblock;
  let started;
  const blocked = new Promise((resolve) => {
    unblock = resolve;
  });
  const entered = new Promise((resolve) => {
    started = resolve;
  });
  const fetcher = async (url, options) => {
    started();
    await blocked;
    return s.fetcher(url, options);
  };
  const args = ["--apply", `--reviewed-sha256=${digest}`];
  const first = run(args, env, () => {}, fetcher);
  await entered;
  try {
    await assert.rejects(
      run(
        args,
        env,
        () => {},
        () => assert.fail("second writer reached API"),
      ),
      /Another operation/,
    );
  } finally {
    unblock();
    await first;
  }
});
