import { expect, test } from "vitest";
import { TerminalControl } from "./TerminalControl";

const owner = (generation = "1") => ({
  supported: true, generation, owned: true, occupied: true, lease_remaining_ms: 90_000,
});

test("input is disabled until an engine observation and on transport loss", () => {
  const control = new TerminalControl();
  expect(control.inputGeneration).toBeUndefined();
  expect(control.observe(owner())).toBe("accepted");
  expect(control.inputGeneration).toBe("1");
  control.disconnect();
  expect(control.inputGeneration).toBeUndefined();
  expect(control.observedGeneration).toBeUndefined();
  expect(control.observe(owner())).toBe("accepted");
  expect(control.inputGeneration).toBe("1");
});

test("a delayed pre-handoff state cannot restore input authority", () => {
  const control = new TerminalControl();
  control.observe(owner("8"));
  control.observe({ ...owner("9"), owned: false });
  expect(control.observe(owner("8"))).toBe("stale");
  expect(control.ownsControl).toBe(false);
  expect(control.observedGeneration).toBe("9");
});

test("engine expiry cannot be undone at the same generation", () => {
  const control = new TerminalControl();
  control.observe(owner("8"));
  control.observe({ ...owner("8"), owned: false, occupied: false, lease_remaining_ms: 0 });
  control.disconnect();
  expect(control.observe(owner("8"))).toBe("stale");
  expect(control.confirmed).toBe(false);
  expect(control.observe(owner("9"))).toBe("accepted");
  expect(control.inputGeneration).toBe("9");
});

test("full-width generations are compared exactly, not as rounded numbers", () => {
  const control = new TerminalControl();
  control.observe(owner("18446744073709551614"));
  control.observe({ ...owner("18446744073709551615"), owned: false });
  expect(control.observe(owner("18446744073709551614"))).toBe("stale");
  expect(control.observedGeneration).toBe("18446744073709551615");
});

test("an unsupported engine stays read-only and does not erase the watermark", () => {
  const control = new TerminalControl();
  control.observe(owner("8"));
  expect(control.observe({ supported: false, generation: null, owned: false, occupied: false, lease_remaining_ms: 0 })).toBe("accepted");
  expect(control.inputGeneration).toBeUndefined();
  expect(control.observedGeneration).toBeUndefined();
  expect(control.observe(owner("7"))).toBe("stale");
  expect(control.observe(owner("8"))).toBe("accepted");
});

test.each([null, [], {}, { ...owner(), generation: 1 }, { ...owner(), generation: "01" },
  { ...owner(), generation: "18446744073709551616" }, { ...owner(), generation: "-1" },
  { ...owner(), generation: "0" }, { ...owner(), occupied: false },
  { ...owner(), lease_remaining_ms: Infinity }, { ...owner(), lease_remaining_ms: 300_001 },
  { ...owner(), supported: false }, { ...owner(), generation: "1e2" },
])("malformed control cannot grant input: %j", (value) => {
  const control = new TerminalControl();
  expect(control.observe(value)).toBe("invalid");
  expect(control.ownsControl).toBe(false);
});

test("an empty initial engine is claimable but does not already own input", () => {
  const control = new TerminalControl();
  expect(control.observe({ supported: true, generation: "0", owned: false, occupied: false, lease_remaining_ms: 0 })).toBe("accepted");
  expect(control.observedGeneration).toBe("0");
  expect(control.inputGeneration).toBeUndefined();
  expect(control.observe(owner("1"))).toBe("accepted");
});

test("invalid control also removes previously confirmed permission", () => {
  const control = new TerminalControl();
  control.observe(owner("8"));
  expect(control.observe({ ...owner("8"), occupied: false })).toBe("invalid");
  expect(control.inputGeneration).toBeUndefined();
  expect(control.observe(owner("7"))).toBe("stale");
});

test("ownership cannot change views without advancing the generation", () => {
  const control = new TerminalControl();
  control.observe({ ...owner("8"), owned: false });
  expect(control.observe(owner("8"))).toBe("invalid");
  expect(control.inputGeneration).toBeUndefined();
  expect(control.observe(owner("9"))).toBe("accepted");
});
