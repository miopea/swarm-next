import { describe, expect, test } from "vitest";

import {
  createApiaryHandoffLink, currentApiaryHandoffLink, readApiaryHandoffLink,
  retargetApiaryHandoffLink, stageApiaryHandoff, takeStagedApiaryHandoff,
} from "./apiaryHandoff";

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

  test("round trips a Keeper capability without placing its secret in the URL path", () => {
    const capability = {
      link_id: "link-1",
      keeper_endpoint: "https://keeper.example.test",
      secret: "private-capability",
    };
    const link = createApiaryHandoffLink("keeper", capability, capability.keeper_endpoint);

    expect(link).toMatch(/^https:\/\/keeper\.example\.test\/#swarm-next-apiary-keeper=/);
    expect(link).not.toContain("private-capability");
    expect(readApiaryHandoffLink(link, "keeper")).toEqual(capability);
  });

  test("retargets a private fragment to an HTTPS personal Hive without sending it in a path or query", () => {
    const link = createApiaryHandoffLink("keeper", { secret: "private-capability" }, "https://keeper.example.test");
    const retargeted = retargetApiaryHandoffLink(link, "https://clover.example.test/settings?old=true", "keeper");

    expect(retargeted).toMatch(/^https:\/\/clover\.example\.test\/#swarm-next-apiary-keeper=/);
    expect(new URL(retargeted).pathname).toBe("/");
    expect(new URL(retargeted).search).toBe("");
    expect(retargeted).not.toContain("private-capability");
    expect(() => retargetApiaryHandoffLink(link, "http://remote.example.test", "keeper")).toThrow(/HTTPS personal Hive/i);
  });

  test("recognizes and stages a current handoff only in memory", () => {
    const link = createApiaryHandoffLink("keeper", { secret: "one-time" }, "https://keeper.example.test");
    const location = new URL(link) as unknown as Location;
    expect(currentApiaryHandoffLink("keeper", location)).toBe(link);
    stageApiaryHandoff("keeper", link);
    expect(takeStagedApiaryHandoff("keeper")).toBe(link);
    expect(takeStagedApiaryHandoff("keeper")).toBeUndefined();
  });
});
