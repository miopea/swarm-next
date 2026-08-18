import { describe, expect, it } from "vitest";

import {
  normalizeRosterQuery,
  orphanSessionMatchesRosterQuery,
  repositoryName,
  workerMatchesRosterQuery,
} from "./workerRoster";

const worker = (name: string, workspace: string) => ({ name, workspace });

describe("repositoryName", () => {
  it("names a worker by the folder it owns", () => {
    expect(repositoryName("/home/operator/projects/rcg/rcg-public-web")).toBe("rcg-public-web");
  });

  it("ignores a trailing separator", () => {
    expect(repositoryName("/home/operator/projects/swarm-next/")).toBe("swarm-next");
  });

  it("keeps an unsplittable workspace visible rather than returning nothing", () => {
    expect(repositoryName("swarm-next")).toBe("swarm-next");
  });
});

describe("normalizeRosterQuery", () => {
  it("treats surrounding whitespace and case as insignificant", () => {
    expect(normalizeRosterQuery("  Public Website \n")).toBe("public website");
  });

  it("reduces a whitespace-only search to no search", () => {
    expect(normalizeRosterQuery("   ")).toBe("");
  });
});

describe("workerMatchesRosterQuery", () => {
  const publicWeb = worker("Public Website", "/home/operator/projects/rcg/rcg-public-web");

  it("matches every worker when there is no search", () => {
    expect(workerMatchesRosterQuery(publicWeb, "")).toBe(true);
  });

  it("matches on the worker name", () => {
    expect(workerMatchesRosterQuery(publicWeb, normalizeRosterQuery("public"))).toBe(true);
  });

  it("matches on the repository name, which is not part of the worker name", () => {
    expect(workerMatchesRosterQuery(publicWeb, normalizeRosterQuery("rcg-public-web"))).toBe(true);
  });

  it("matches on an interior path segment", () => {
    expect(workerMatchesRosterQuery(publicWeb, normalizeRosterQuery("projects/rcg"))).toBe(true);
  });

  it("does not match unrelated text", () => {
    expect(workerMatchesRosterQuery(publicWeb, normalizeRosterQuery("budgetbug"))).toBe(false);
  });

  it("separates adjacent fields so a search cannot span the join", () => {
    // "Website" ends the name and "/home" starts the workspace; without the
    // separator "websitehome" would match and surprise the operator.
    expect(workerMatchesRosterQuery(publicWeb, normalizeRosterQuery("websitehome"))).toBe(false);
  });
});

describe("orphanSessionMatchesRosterQuery", () => {
  it("matches every session when there is no search", () => {
    expect(orphanSessionMatchesRosterQuery("Session 4f2a", "")).toBe(true);
  });

  it("matches the session name", () => {
    expect(orphanSessionMatchesRosterQuery("Session 4f2a", normalizeRosterQuery("4f2a"))).toBe(true);
  });

  it("matches the label an operator can actually see", () => {
    expect(orphanSessionMatchesRosterQuery("Session 4f2a", normalizeRosterQuery("unconfigured"))).toBe(true);
  });

  it("does not match unrelated text", () => {
    expect(orphanSessionMatchesRosterQuery("Session 4f2a", normalizeRosterQuery("queen"))).toBe(false);
  });
});
