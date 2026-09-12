import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, rmdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const OWNER = "[keeper-ops:v1] ";
const MAX_BYTES = 2 * 1024 * 1024;
const CATEGORIES = ["diagnostics", "productAnalytics"];

export class SafeError extends Error {}
class RequestTimeout extends SafeError {}
const fail = (message) => {
  throw new SafeError(message);
};
const object = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const id = (value) => {
  const text = String(value);
  if (!/^[a-zA-Z0-9_-]{1,128}$/.test(text)) fail("Invalid resource identifier.");
  return text;
};

export async function loadManifest() {
  const bytes = await readFile(new URL("../../deploy/posthog/manifest.json", import.meta.url));
  const manifest = JSON.parse(bytes.toString("utf8"));
  validateManifest(manifest);
  return { manifest, digest: createHash("sha256").update(bytes).digest("hex") };
}

export function validateManifest(m) {
  if (
    m.version !== 1 ||
    m.governance?.metricStatus !== "proposed" ||
    m.governance.aiProcessing !== false ||
    m.governance.personEnrichment !== false ||
    m.governance.humanReviewRequired !== true ||
    m.marketing?.publishSourceMappings !== false
  ) {
    fail("Manifest violates governance policy.");
  }
  if (
    m.target?.host !== "https://us.posthog.com" ||
    m.target.ingestHost !== "https://us.i.posthog.com" ||
    m.target.projectId !== "605256" ||
    m.target.projectName !== "keeper"
  )
    fail("Manifest target is not Keeper.");
  if (
    m.remoteConfig?.key !== "keeper-client-config" ||
    typeof m.remoteConfig.supportMessage !== "string" ||
    m.remoteConfig.supportMessage.length > 200 ||
    /[<>\r\n]|https?:|www\./i.test(m.remoteConfig.supportMessage)
  )
    fail("Invalid public configuration.");
  const expected = [
    "keeper_app_ready",
    "keeper_command_palette_opened",
    "keeper_settings_opened",
    "keeper_frontend_error",
    "keeper_interaction",
  ];
  if (
    !Array.isArray(m.events) ||
    m.events.length !== expected.length ||
    m.events.some((e, i) => e.name !== expected[i] || !CATEGORIES.includes(e.category))
  )
    fail("Invalid event catalog.");
  for (const resource of [...m.personas, ...m.metrics, m.dashboard]) id(resource.name);
  if (
    m.personas.some(
      (persona) =>
        !m.events.some(
          (event) => event.name === persona.event && event.category === "productAnalytics",
        ),
    )
  )
    fail("Persona event must belong to the productAnalytics catalog.");
  // Sanity checks only, not a SQL parser or a privacy/authorization boundary.
  // Human review of every query and the exact reviewed manifest hash are the control.
  for (const metric of m.metrics) {
    if (
      !/^[A-Za-z][A-Za-z0-9_]*$/.test(metric.name) ||
      !metric.sql.startsWith("SELECT ") ||
      !metric.sql.includes("properties.synthetic = false") ||
      !metric.sql.includes("consent_category")
    ) {
      fail("Invalid metric definition.");
    }
  }
  if (m.marketing.event !== "keeper_settings_opened") fail("Invalid conversion definition.");
}

export function configuration(env, manifest, needsKey = true) {
  // No defaults: even the known project must be deliberately selected by the operator.
  if (
    env.POSTHOG_HOST !== manifest.target.host ||
    env.POSTHOG_PROJECT_ID !== manifest.target.projectId
  ) {
    fail("Set POSTHOG_HOST and POSTHOG_PROJECT_ID to the reviewed Keeper target.");
  }
  if (
    env.CI &&
    (env.GITHUB_EVENT_NAME !== "workflow_dispatch" ||
      env.GITHUB_REF !== "refs/heads/main" ||
      env.GITHUB_REPOSITORY !== "tgorka/keeper")
  ) {
    fail("Privileged tooling requires trusted manual main execution.");
  }
  const key = env.POSTHOG_PERSONAL_API_KEY;
  if (needsKey && (typeof key !== "string" || !/^phx_[A-Za-z0-9_-]+$/.test(key))) {
    fail("Set POSTHOG_PERSONAL_API_KEY in the maintainer process only.");
  }
  return { host: manifest.target.host, projectId: manifest.target.projectId, key };
}

export function safeMessage(error) {
  // Never emit remote messages, exception causes, URLs, credentials or stack traces.
  return error instanceof SafeError
    ? error.message
    : "PostHog operation failed; details suppressed.";
}

export class Api {
  #config;
  #fetch;
  constructor(config, fetcher = fetch) {
    this.#config = config;
    this.#fetch = fetcher;
  }
  get root() {
    return `/api/projects/${this.#config.projectId}/`;
  }

  async request(path, method = "GET", body, publicHost) {
    const origin = publicHost ?? this.#config.host;
    const url = new URL(path, origin);
    // Pagination URLs must not redirect credentials across origin, project or endpoint.
    if (
      url.origin !== origin ||
      url.username ||
      url.password ||
      url.hash ||
      (!publicHost && !url.pathname.startsWith(this.root)) ||
      (publicHost &&
        (publicHost !== "https://us.i.posthog.com" ||
          !["/i/v0/e/", "/flags/"].includes(url.pathname)))
    ) {
      fail("Refused unsafe API destination.");
    }
    const deadline = AbortSignal.timeout(15000);
    try {
      const response = await this.#fetch(url, {
        method,
        headers: {
          "Content-Type": "application/json",
          ...(!publicHost ? { Authorization: `Bearer ${this.#config.key}` } : {}),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: deadline,
        redirect: "error",
      });
      if (!response.ok) fail(`PostHog HTTP ${response.status}; response suppressed.`);
      const reader = response.body?.getReader();
      if (!reader) fail("PostHog returned an empty response.");
      const chunks = [];
      let size = 0;
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        size += value.byteLength;
        if (size > MAX_BYTES) {
          await reader.cancel();
          fail("PostHog response exceeded size limit.");
        }
        chunks.push(value);
      }
      return JSON.parse(Buffer.concat(chunks).toString("utf8"));
    } catch (error) {
      if (error instanceof SafeError) throw error;
      if (deadline.aborted || error?.name === "TimeoutError")
        throw new RequestTimeout("PostHog request deadline exceeded; details suppressed.");
      fail(
        "PostHog request failed; details suppressed. No automatic write retry; run plan before retrying.",
      );
    }
  }

  async list(collection) {
    const base = `${this.root}${collection}/`;
    let next = `${base}?limit=100`;
    const seen = new Set();
    const rows = [];
    for (let page = 0; next && page < 100; page++) {
      const url = new URL(next, this.#config.host);
      if (url.pathname !== base || url.origin !== this.#config.host || seen.has(url.href))
        fail("Unsafe or repeated pagination URL.");
      seen.add(url.href);
      const data = await this.request(url.href);
      if (
        !object(data) ||
        !Array.isArray(data.results) ||
        (data.next != null && typeof data.next !== "string")
      ) {
        fail("Unexpected PostHog list response.");
      }
      rows.push(...data.results);
      next = data.next;
    }
    if (next) fail("PostHog pagination limit exceeded.");
    return rows;
  }
}

const filter = (category) => [
  { key: "synthetic", value: ["false"], operator: "exact", type: "event" },
  { key: "consent_category", value: [category], operator: "exact", type: "event" },
];

export function resources(m) {
  const result = [];
  const add = (collection, name, body, extra = {}) =>
    result.push({ collection, name, body, ...extra });
  for (const event of m.events)
    add("event_definitions", event.name, {
      name: event.name,
      description: OWNER + event.description,
      tags: ["keeper-ops-v1", event.category.toLowerCase()],
    });
  add(
    "feature_flags",
    m.remoteConfig.key,
    {
      key: m.remoteConfig.key,
      name: `${OWNER}Public non-secret support message only; never consent or security policy.`,
      active: true,
      is_remote_configuration: true,
      evaluation_runtime: "all",
      filters: {
        groups: [{ properties: [], rollout_percentage: 100 }],
        payloads: { true: JSON.stringify({ supportMessage: m.remoteConfig.supportMessage }) },
      },
      tags: ["keeper-ops-v1"],
    },
    { identity: "key", ownerField: "name" },
  );
  for (const persona of m.personas)
    add("cohorts", persona.name, {
      name: persona.name,
      description: OWNER + persona.description,
      is_static: false,
      filters: {
        properties: {
          type: "AND",
          values: [
            {
              type: "OR",
              values: [
                {
                  type: "behavioral",
                  key: "performed_event",
                  value: persona.event,
                  event_type: "events",
                  time_value: 30,
                  time_interval: "day",
                  event_filters: filter("productAnalytics"),
                },
              ],
            },
          ],
        },
      },
    });
  add("dashboards", m.dashboard.name, {
    name: m.dashboard.name,
    description: OWNER + m.dashboard.description,
    tags: ["keeper-ops-v1"],
  });
  for (const metric of m.metrics) {
    const query = { kind: "HogQLQuery", query: metric.sql };
    add(
      "data_catalog/metrics",
      metric.name,
      {
        name: metric.name,
        display_name: metric.displayName,
        description: OWNER + metric.description,
        unit: metric.unit,
        definition: query,
        created_source: "ai_generated",
        ai_model: "openai-codex/gpt-6-astra",
        confidence: null,
        reasoning:
          "Proposed over closed events; SQL substring checks are sanity only. Human SQL review and the reviewed manifest hash control consent/synthetic filtering. Maintainer must evaluate coverage and approve separately; no calibrated confidence is available.",
      },
      { addressByName: true, governed: true },
    );
    add(
      "insights",
      metric.name,
      {
        name: metric.name,
        description: OWNER + metric.description.slice(0, 370),
        query: { kind: "DataTableNode", source: query },
        tags: ["keeper-ops-v1", "proposed"],
      },
      { dashboard: m.dashboard.name },
    );
    add(
      "endpoints",
      metric.name,
      {
        name: metric.name,
        description: OWNER + metric.description,
        query,
        is_active: true,
        is_materialized: false,
        data_freshness_seconds: 900,
      },
      { addressByName: true },
    );
  }
  return result;
}

// Readback may add defaults to query ASTs. Compare the declared contract recursively,
// preserving ordered query arrays. PostHog tags are lowercase unordered labels.
export function contains(actual, desired) {
  if (Array.isArray(desired))
    return (
      Array.isArray(actual) &&
      actual.length === desired.length &&
      desired.every((v, i) => contains(actual[i], v))
    );
  if (object(desired))
    return (
      object(actual) &&
      Object.entries(desired).every(([k, v]) => {
        if (k === "tags" && Array.isArray(v) && Array.isArray(actual[k])) {
          return contains([...actual[k]].sort(), [...v].sort());
        }
        return contains(actual[k], v);
      })
    );
  return actual === desired;
}

function resourcePath(api, resource, row) {
  return `${api.root}${resource.collection}/${id(resource.addressByName ? resource.name : row.id)}/`;
}

async function inspect(api, m) {
  const project = await api.request(api.root);
  if (String(project.id) !== m.target.projectId || project.name !== m.target.projectName)
    fail("Project identity mismatch.");
  const planned = resources(m);
  const collections = new Map();
  // Finish all capability and ownership checks before the first write.
  for (const collection of new Set(planned.map((r) => r.collection)))
    collections.set(collection, await api.list(collection));
  for (const resource of planned) {
    const matches = collections
      .get(resource.collection)
      .filter((row) => row[resource.identity ?? "name"] === resource.name);
    if (matches.length > 1) fail("Duplicate managed resource; resolve manually before applying.");
    if (matches.length === 1) {
      const row = await api.request(resourcePath(api, resource, matches[0]));
      if (
        row.deleted ||
        row.archived ||
        !String(row[resource.ownerField ?? "description"]).startsWith(OWNER)
      ) {
        fail("Managed name collides with an unowned or archived resource; review manually.");
      }
      if (resource.collection === "dashboards" && row.is_shared)
        fail("Managed dashboard is publicly shared; disable sharing manually.");
      resource.current = row;
      if (resource.governed && row.status !== "proposed" && row.status !== "approved")
        fail("Unknown metric governance state.");
      if (
        resource.governed &&
        row.status === "approved" &&
        (!contains(row, resource.body) || row.is_drifted)
      ) {
        fail("Approved metric drift requires human review; automation will not overwrite it.");
      }
    }
  }
  const marketing = project.marketing_analytics_config;
  if (!object(marketing)) fail("Marketing configuration unavailable; check beta access.");
  const goals = marketing.conversion_goals ?? [];
  if (!Array.isArray(goals)) fail("Unexpected marketing goal configuration.");
  // Check availability without retrieving raw conversion-event samples.
  await api.request(`${api.root}marketing_analytics/conversion_goals/`);
  const goalMatches = goals.filter((g) => g.conversion_goal_name === m.marketing.conversionName);
  if (goalMatches.length > 1) fail("Duplicate conversion goal; resolve manually.");
  const goal = {
    conversion_goal_name: m.marketing.conversionName,
    kind: "EventsNode",
    event: m.marketing.event,
    math: "total",
    properties: filter("productAnalytics"),
    counts_as_customer: false,
    counts_as_revenue: false,
    schema_map: {},
  };
  // Goals do not expose description/tags: adopt only an exact contract, never edit a collision.
  if (goalMatches.length && !contains(goalMatches[0], goal))
    fail("Conversion goal drift requires human review.");
  return { planned, goal, currentGoal: goalMatches[0] };
}

export async function reconcile(api, m, mode, emit = console.log) {
  const { planned, goal, currentGoal } = await inspect(api, m);
  const dashboard = planned.find((r) => r.collection === "dashboards");
  let dashboardId = dashboard.current?.id;
  let drift = false;
  for (const resource of planned) {
    const body = { ...resource.body };
    const attached =
      !resource.dashboard ||
      (dashboardId !== undefined &&
        resource.current?.dashboard_tiles?.some((t) => t.dashboard_id === dashboardId));
    const same = resource.current && contains(resource.current, body) && attached;
    const action = same ? "unchanged" : resource.current ? "update" : "create";
    emit(`${action} ${resource.collection} ${resource.name}`);
    if (same) continue;
    drift = true;
    if (mode !== "apply") continue;
    // The documented dashboards write field is still supported; read association via dashboard_tiles.
    // Preserve every other dashboard when adding the managed one.
    if (resource.dashboard) {
      if (dashboardId === undefined) fail("Missing managed dashboard.");
      body.dashboards = [
        ...new Set([
          ...(resource.current?.dashboard_tiles ?? []).map((t) => t.dashboard_id),
          dashboardId,
        ]),
      ];
    }
    const path = resource.current
      ? resourcePath(api, resource, resource.current)
      : `${api.root}${resource.collection}/`;
    const created = await api.request(path, resource.current ? "PATCH" : "POST", body);
    const readback = await api.request(resourcePath(api, resource, created));
    if (!contains(readback, resource.body)) fail("Managed resource readback mismatch.");
    if (resource.governed && readback.status !== "proposed")
      fail("New or changed metric was not proposed.");
    if (resource.collection === "dashboards") dashboardId = readback.id;
    if (
      resource.dashboard &&
      !readback.dashboard_tiles?.some((t) => t.dashboard_id === dashboardId)
    )
      fail("Dashboard association readback mismatch.");
  }
  emit(`${currentGoal ? "unchanged" : "create"} conversion_goal ${m.marketing.conversionName}`);
  if (!currentGoal) {
    drift = true;
    if (mode === "apply") {
      // Server allocates the ID. No project-wide marketing config patch or ad integration.
      await api.request(`${api.root}marketing_analytics/conversion_goals/create/`, "POST", {
        goal,
      });
      const project = await api.request(api.root);
      const goals = project.marketing_analytics_config?.conversion_goals;
      if (!Array.isArray(goals) || goals.filter((g) => contains(g, goal)).length !== 1)
        fail("Conversion goal readback mismatch.");
    }
  }
  if (mode === "verify" && drift) fail("Managed resources differ from manifest.");
  return { drift };
}

export async function smoke(
  api,
  m,
  env,
  emit = console.log,
  pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
) {
  // Smoke is an explicit ingestion effect, not a dry run or a resource approval.
  const token = env.POSTHOG_PROJECT_API_KEY;
  if (
    env.POSTHOG_INGEST_HOST !== m.target.ingestHost ||
    typeof token !== "string" ||
    !/^phc_[A-Za-z0-9_-]+$/.test(token)
  )
    fail("Set explicit Keeper ingestion host and project token for synthetic smoke.");
  const project = await api.request(api.root);
  if (
    String(project.id) !== m.target.projectId ||
    project.name !== m.target.projectName ||
    project.api_token !== token
  )
    fail("Synthetic smoke project token mismatch.");
  const uuid = randomUUID();
  const distinctId = `keeper-ops-smoke-${uuid}`;
  const flags = await api.request(
    "/flags/",
    "POST",
    { api_key: token, distinct_id: distinctId, send_feature_flag_events: false },
    m.target.ingestHost,
  );
  let payload =
    flags.featureFlagPayloads?.[m.remoteConfig.key] ??
    flags.flags?.[m.remoteConfig.key]?.metadata?.payload;
  if (typeof payload === "string") {
    try {
      payload = JSON.parse(payload);
    } catch {
      fail("Remote config smoke payload invalid.");
    }
  }
  if (!contains(payload, { supportMessage: m.remoteConfig.supportMessage }))
    fail("Remote config public readback mismatch.");
  const captured = await api.request(
    "/i/v0/e/",
    "POST",
    {
      api_key: token,
      event: "keeper_ops_smoke",
      uuid,
      properties: {
        distinct_id: distinctId,
        synthetic: true,
        event_source: "maintainer_smoke",
        $process_person_profile: false,
        $geoip_disable: true,
      },
    },
    m.target.ingestHost,
  );
  if (captured.status !== 1 && captured.status !== "Ok") fail("Synthetic capture not accepted.");
  if (Array.isArray(captured.quota_limited) && captured.quota_limited.length)
    fail("Synthetic capture is quota limited.");
  let found = false;
  for (let attempt = 0; attempt < 12; attempt++) {
    // Locally generated UUID contains no SQL metacharacters; never interpolate user input.
    try {
      const response = await api.request(`${api.root}query/`, "POST", {
        query: {
          kind: "HogQLQuery",
          query: `SELECT count() FROM events WHERE event = 'keeper_ops_smoke' AND uuid = '${uuid}' AND properties.synthetic = true AND timestamp >= now() - INTERVAL 1 HOUR`,
        },
        refresh: "force_blocking",
      });
      if (Number(response.results?.[0]?.[0]) === 1) {
        found = true;
        break;
      }
    } catch (error) {
      // Only this read-only observation may retry a deadline. Never resend ingestion
      // or resource writes, whose effect is ambiguous after a transport failure.
      if (!(error instanceof RequestTimeout)) throw error;
    }
    if (attempt < 11) await pause(5000);
  }
  if (!found) fail("Synthetic event not observed within the bounded ingestion window.");
  emit("verified synthetic_ingestion keeper_ops_smoke");
  for (const metric of m.metrics) {
    const response = await api.request(`${api.root}endpoints/${id(metric.name)}/run/`, "POST", {});
    if (!Array.isArray(response.results))
      fail("Analytical endpoint returned an unexpected result.");
    emit(`verified endpoint ${metric.name}`);
  }
  emit("Synthetic smoke complete; no application or person content sent.");
}

export async function run(args, env, emit = console.log, fetcher = fetch) {
  const { manifest, digest } = await loadManifest();
  const modes = args.filter((a) =>
    ["--dry-run", "--plan", "--apply", "--verify", "--smoke"].includes(a),
  );
  const digestArgs = args.filter((a) => a.startsWith("--reviewed-sha256="));
  if (args.some((a) => !modes.includes(a) && !digestArgs.includes(a)) || digestArgs.length > 1)
    fail("Unknown or duplicate CLI arguments.");
  const smokeMode = modes.includes("--smoke");
  if (
    new Set(modes).size !== modes.length ||
    (smokeMode ? modes.length !== 2 || !modes.includes("--apply") : modes.length > 1)
  )
    fail("Choose one mode; synthetic smoke requires --smoke --apply.");
  const mode = smokeMode ? "smoke" : (modes[0] ?? "--dry-run").slice(2);
  const config = configuration(env, manifest, mode !== "dry-run");
  if ((mode === "apply" || mode === "smoke") && digestArgs[0] !== `--reviewed-sha256=${digest}`)
    fail("Review the manifest and pass its exact --reviewed-sha256 before mutation.");
  emit(`manifest sha256 ${digest}`);
  if (mode === "dry-run") {
    for (const r of resources(manifest)) emit(`desired ${r.collection} ${r.name}`);
    emit(`desired conversion_goal ${manifest.marketing.conversionName}`);
    for (const mapping of manifest.marketing.sourceMappings)
      emit(`boundary source_mapping ${mapping.source}`);
    emit(
      "Dry run: no network, no secrets required, no mutation. Use --plan for read-only remote comparison.",
    );
    return;
  }
  const execute = async () => {
    const api = new Api(config, fetcher);
    if (mode === "smoke") {
      await reconcile(api, manifest, "verify", emit);
      await smoke(api, manifest, env, emit);
    } else {
      await reconcile(api, manifest, mode, emit);
      if (mode === "apply") await reconcile(api, manifest, "verify", emit);
    }
  };
  if (mode !== "apply" && mode !== "smoke") return execute();
  // Prevent local writers racing a list-then-create API. CI also serializes via
  // its concurrency group. Cross-host operators must not apply concurrently.
  const lock = join(tmpdir(), "keeper-posthog-605256.lock");
  try {
    await mkdir(lock, { mode: 0o700 });
  } catch {
    fail(
      "Another operation or stale keeper-posthog-605256.lock exists in the OS temp directory; investigate before removing it.",
    );
  }
  try {
    return await execute();
  } finally {
    await rmdir(lock);
  }
}
