import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { keyBackupStore } from "@/lib/stores/key-backup";

beforeEach(() => {
  keyBackupStore.getState().reset();
});

afterEach(() => {
  keyBackupStore.getState().reset();
});

describe("keyBackupStore", () => {
  it("records and removes per-account statuses", () => {
    keyBackupStore.getState().setStatus("acc-1", "disabled");
    keyBackupStore.getState().setStatus("acc-2", "enabled");
    expect(keyBackupStore.getState().statuses).toEqual({
      "acc-1": "disabled",
      "acc-2": "enabled",
    });

    keyBackupStore.getState().removeAccount("acc-1");
    expect(keyBackupStore.getState().statuses).toEqual({ "acc-2": "enabled" });
    // Removing an unknown account is a no-op.
    keyBackupStore.getState().removeAccount("nope");
    expect(keyBackupStore.getState().statuses).toEqual({ "acc-2": "enabled" });
  });

  it("openEnable opens the modal in enable mode with a clean action state", () => {
    keyBackupStore.getState().setRecoveryKey("old-key");
    keyBackupStore.getState().openEnable("acc-1");
    const s = keyBackupStore.getState();
    expect(s.modalOpen).toBe(true);
    expect(s.mode).toBe("enable");
    expect(s.accountId).toBe("acc-1");
    expect(s.recoveryKey).toBeNull();
    expect(s.phase).toBe("idle");
    expect(s.error).toBeNull();
  });

  it("openRestore opens the modal in restore mode with a clean action state", () => {
    keyBackupStore.getState().openRestore("acc-1");
    const s = keyBackupStore.getState();
    expect(s.modalOpen).toBe(true);
    expect(s.mode).toBe("restore");
    expect(s.accountId).toBe("acc-1");
    expect(s.phase).toBe("idle");
  });

  it("setRecoveryKey records the key and moves to the shown phase", () => {
    keyBackupStore.getState().openEnable("acc-1");
    keyBackupStore.getState().setRecoveryKey("EsT_recovery_key");
    const s = keyBackupStore.getState();
    expect(s.recoveryKey).toBe("EsT_recovery_key");
    expect(s.phase).toBe("shown");
  });

  it("setError records the named code and moves to the failed phase", () => {
    keyBackupStore.getState().openRestore("acc-1");
    keyBackupStore.getState().setError("backupIncorrectKey");
    const s = keyBackupStore.getState();
    expect(s.error).toBe("backupIncorrectKey");
    expect(s.phase).toBe("failed");
  });

  it("switchToRestore flips the mode and clears the transient state", () => {
    keyBackupStore.getState().openEnable("acc-1");
    keyBackupStore.getState().setRecoveryKey("key");
    keyBackupStore.getState().switchToRestore();
    const s = keyBackupStore.getState();
    expect(s.mode).toBe("restore");
    expect(s.recoveryKey).toBeNull();
    expect(s.phase).toBe("idle");
    // The account and open state are preserved so the restore modal stays up.
    expect(s.accountId).toBe("acc-1");
    expect(s.modalOpen).toBe(true);
  });

  it("close clears the recovery key and all modal state", () => {
    keyBackupStore.getState().openEnable("acc-1");
    keyBackupStore.getState().setRecoveryKey("secret-recovery-key");
    keyBackupStore.getState().close();
    const s = keyBackupStore.getState();
    expect(s.modalOpen).toBe(false);
    expect(s.mode).toBeNull();
    expect(s.accountId).toBeNull();
    // The recovery key must never outlive the open modal.
    expect(s.recoveryKey).toBeNull();
    expect(s.phase).toBe("idle");
    expect(s.error).toBeNull();
  });
});
