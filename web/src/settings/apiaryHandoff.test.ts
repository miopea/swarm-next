import { describe, expect, test } from "vitest";

import { createApiaryHandoffLink, readApiaryHandoffLink } from "./apiaryHandoff";

describe("Apiary handoff links", () => {
  test("round trips Unicode content without exposing it outside the URL fragment", () => {
    const payload = { hive: "Clover Hive 🐝", secret: "one-time" };
    const link = createApiaryHandoffLink("invitation", payload, "https://swarm.example.test/settings?old=true");

    expect(link).toMatch(/^https:\/\/swarm\.example\.test\/#swarm-next-apiary-invitation=/);
    expect(link).not.toContain("one-time");
    expect(readApiaryHandoffLink(link, "invitation")).toEqual(payload);
  });

  test("rejects the wrong handoff type and incomplete values", () => {
    const link = createApiaryHandoffLink("connection", { hive: "Clover" }, "https://swarm.example.test");
    expect(() => readApiaryHandoffLink(link, "invitation")).toThrow(/not a Swarm Apiary invitation link/i);
    expect(() => readApiaryHandoffLink("not a link", "connection")).toThrow(/complete Swarm Apiary link/i);
  });
});
