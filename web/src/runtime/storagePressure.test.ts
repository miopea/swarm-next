import { expect, test } from "vitest";
import type { StorageObservation } from "../api";
import { storageLabel, storagePressureNotice } from "./storagePressure";

function sample(pressure: StorageObservation["pressure"], available: number | null = 1024 ** 3): StorageObservation {
  return { scope: "system", pressure, available_bytes: available, total_bytes: 61 * 1024 ** 3,
    advisory_available_bytes: 2 * 1024 ** 3, critical_available_bytes: 512 * 1024 ** 2 };
}
test("normal recovery is quiet, low storage is explicit and readings are not summed", () => {
  expect(storagePressureNotice([sample("normal")])).toBeNull();
  const low = storagePressureNotice([sample("advisory"), { ...sample("critical"), scope: "database" }]);
  expect(low?.level).toBe("critical");
  expect(low?.detail).toContain("System storage");
  expect(low?.detail).toContain("Hive storage");
  expect(low?.detail).toContain("do not add");
  expect(storagePressureNotice([sample("normal", 20 * 1024 ** 3)])).toBeNull();
});
test("missing readings are not fabricated as zero free or healthy", () => {
  expect(storagePressureNotice([sample("unavailable", null)])?.level).toBe("unknown");
  expect(storageLabel(sample("unavailable", null))).toBe("System storage: unavailable");
  expect(storagePressureNotice(undefined)).toBeNull();
});
