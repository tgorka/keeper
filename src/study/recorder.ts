import posthog from "posthog-js/dist/module.no-external";
import "posthog-js/dist/posthog-recorder";
import type { CaptureResult } from "posthog-js";
import type { TelemetryStudyConfigVm } from "@/lib/ipc/gen/TelemetryStudyConfigVm";

const STUDY_URL = "https://keeper.invalid/synthetic-study";

/** Drop every event/property except what the synthetic replay and heatmap renderer needs. */
export function studyEvent(
  event: CaptureResult | null,
  projectToken: string,
): CaptureResult | null {
  if (!event || !["$snapshot", "$$heatmap", "$pageview"].includes(event.event)) return null;
  const properties: CaptureResult["properties"] = {
    token: projectToken,
    distinct_id: event.properties.distinct_id,
    $session_id: event.properties.$session_id,
    $window_id: event.properties.$window_id,
    $current_url: STUDY_URL,
    $process_person_profile: false,
    $geoip_disable: true,
    $ip: "0.0.0.0",
  };
  if (event.event === "$snapshot") {
    properties.$snapshot_data = event.properties.$snapshot_data;
    properties.$snapshot_bytes = event.properties.$snapshot_bytes;
  }
  if (event.event === "$$heatmap") {
    const width: unknown = event.properties.$viewport_width;
    const height: unknown = event.properties.$viewport_height;
    if (
      typeof width !== "number" ||
      !Number.isFinite(width) ||
      width <= 0 ||
      typeof height !== "number" ||
      !Number.isFinite(height) ||
      height <= 0
    )
      return null;
    properties.$viewport_width = width;
    properties.$viewport_height = height;
    const heatmaps: unknown = event.properties.$heatmap_data;
    if (!heatmaps || typeof heatmaps !== "object") return null;
    const points = Object.values(heatmaps)
      .flat()
      .flatMap((point: unknown) => {
        if (
          !point ||
          typeof point !== "object" ||
          !("x" in point) ||
          !("y" in point) ||
          !("type" in point) ||
          !("target_fixed" in point) ||
          typeof point.x !== "number" ||
          !Number.isFinite(point.x) ||
          typeof point.y !== "number" ||
          !Number.isFinite(point.y) ||
          (point.type !== "click" && point.type !== "mousemove") ||
          typeof point.target_fixed !== "boolean"
        )
          return [];
        return [{ x: point.x, y: point.y, type: point.type, target_fixed: point.target_fixed }];
      });
    properties.$heatmap_data = { [STUDY_URL]: points };
  }
  return { uuid: event.uuid, event: event.event, timestamp: event.timestamp, properties };
}

export interface StudyRecorder {
  start(): void;
  recording(): boolean;
  ready(): void;
  stop(): Promise<void>;
}

export function createStudyRecorder(
  config: TelemetryStudyConfigVm,
  onFailure: () => void,
): StudyRecorder {
  const remoteConfigUrl = new URL(`/array/${config.projectToken}/config`, config.host);
  let active = true;
  let remainingBytes = 8 * 1024 * 1024;
  let remainingEvents = 2_000;
  const fail = () => {
    if (!active) return;
    active = false;
    onFailure();
  };
  return {
    start() {
      posthog.init(config.projectToken, {
        api_host: config.host,
        // asset_host only overrides /static/*, not regional /array/* config requests.
        // The supported rewrite hook keeps that JSON request on the disclosed origin.
        rewriteRequestPath: (url) =>
          url.pathname === remoteConfigUrl.pathname ? remoteConfigUrl : url,
        api_transport: "fetch",
        defaults: "2026-01-30",
        persistence: "memory",
        disable_persistence: true,
        bootstrap: { distinctID: crypto.randomUUID(), isIdentifiedID: false },
        person_profiles: "never",
        autocapture: false,
        capture_pageview: false,
        capture_pageleave: false,
        capture_exceptions: false,
        capture_performance: false,
        capture_heatmaps: true,
        capture_dead_clicks: false,
        rageclick: false,
        enable_recording_console_log: false,
        disable_external_dependency_loading: true,
        disable_surveys: true,
        disable_product_tours: true,
        disable_conversations: true,
        opt_in_site_apps: false,
        advanced_disable_feature_flags: true,
        advanced_disable_feature_flags_on_first_load: true,
        remote_config_refresh_interval_ms: 0,
        save_campaign_params: false,
        save_referrer: false,
        custom_campaign_params: [],
        disable_session_recording: true,
        session_recording: {
          maskAllInputs: true,
          maskTextSelector: "*",
          maskAllElementAttributes: true,
          captureCanvas: { recordCanvas: false },
          blockSelector:
            "#study-controls, input, textarea, select, iframe, img, video, audio, canvas, script",
          recordHeaders: false,
          recordBody: false,
          // Remote replay toggles can install the network plugin. This unconditional veto
          // is the privacy boundary, paired with capture_performance:false (no initial scan).
          maskCapturedNetworkRequestFn: () => null,
          recordCrossOriginIframes: false,
          collectFonts: false,
          captureJsonLd: false,
          inlineStylesheet: true,
        },
        before_send: (event) => {
          if (!active) return null;
          const safe = studyEvent(event, config.projectToken);
          if (!safe) return null;
          try {
            // Three UTF-8 bytes per UTF-16 code unit is a conservative upper bound.
            const bytes = JSON.stringify(safe).length * 3;
            if (remainingEvents <= 0 || bytes > 512 * 1024 || bytes > remainingBytes) {
              fail();
              return null;
            }
            remainingBytes -= bytes;
            remainingEvents--;
            return safe;
          } catch {
            fail();
            return null;
          }
        },
        request_queue_config: { flush_interval_ms: 1_000 },
        // Never inspect or print response bodies, request metadata, or SDK errors.
        on_request_error: fail,
      });
      posthog.startSessionRecording(true);
    },
    recording: () => posthog.sessionRecordingStarted(),
    ready: () => posthog.capture("$pageview"),
    stop: async () => {
      active = false;
      posthog.set_config({ capture_heatmaps: false });
      posthog.stopSessionRecording();
      // Caller has already closed the transport fence. shutdown may flush its queues;
      // no such flush, beacon, retry, or in-flight request may outlive this session.
      await posthog.shutdown();
    },
  };
}
