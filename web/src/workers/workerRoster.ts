import type { Worker } from "../api";

/**
 * The trailing folder of a workspace path. Operators name workers after the
 * repository they own, so this is what a roster search is expected to match.
 */
export function repositoryName(workspace: string): string {
  return workspace.split(/[\\/]/).filter(Boolean).at(-1) ?? workspace;
}

/**
 * Reduces operator-typed search text to a comparable needle. An empty needle
 * means "no search", which every matcher below treats as matching everything.
 */
export function normalizeRosterQuery(query: string): string {
  return query.trim().toLocaleLowerCase();
}

/**
 * The one matching rule for the worker roster. The desktop rail and the mobile
 * switcher share it so the same text cannot select different workers depending
 * on which surface the operator happens to be using.
 */
export function workerMatchesRosterQuery(
  worker: Pick<Worker, "name" | "workspace">,
  needle: string,
): boolean {
  if (!needle) return true;
  return `${worker.name} ${repositoryName(worker.workspace)} ${worker.workspace}`
    .toLocaleLowerCase()
    .includes(needle);
}

/**
 * Matching rule for a running session that has no roster profile yet. It is
 * kept beside the profile rule because both answer one operator question.
 */
export function orphanSessionMatchesRosterQuery(name: string, needle: string): boolean {
  if (!needle) return true;
  return `${name} unconfigured session`.toLocaleLowerCase().includes(needle);
}
