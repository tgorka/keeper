import { invoke } from "@tauri-apps/api/core";
import type { TelemetryStudyConfigVm } from "@/lib/ipc/gen/TelemetryStudyConfigVm";
import type { TelemetryStudyPreviewVm } from "@/lib/ipc/gen/TelemetryStudyPreviewVm";

// This entry intentionally imports no app client, store, account, or real-data component.
export function telemetryStudyPreview(): Promise<TelemetryStudyPreviewVm> {
  return invoke("telemetry_study_preview");
}

export function telemetryStudyConfig(): Promise<TelemetryStudyConfigVm | null> {
  return invoke("telemetry_study_config");
}

export function telemetryStudyStop(): Promise<void> {
  return invoke("telemetry_study_stop");
}
