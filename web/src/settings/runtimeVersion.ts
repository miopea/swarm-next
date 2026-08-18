export function compactRuntimeVersion(version: string) {
  const revision = developmentRevision(version);
  const release = version.split("-dev-")[0];
  return revision ? `Healthy · ${release} · ${revision}` : `Healthy · ${version}`;
}

export function runtimeVersionIdentity(version?: string | null) {
  if (!version) return "Unavailable";
  const revision = developmentRevision(version);
  const release = version.split("-dev-")[0];
  return revision ? `${release} · revision ${revision}` : version;
}

export function deployedRevision(version: string) {
  return version.match(/-(?:dev-)?([0-9a-f]{7,40})(?:-|$)/i)?.[1]?.slice(0, 7) ?? "the current build";
}

export function shortRevision(revision?: string | null) {
  return revision?.slice(0, 7);
}

function developmentRevision(version: string) {
  return version.match(/-dev-([0-9a-f]{7,40})(?:-|$)/i)?.[1]?.slice(0, 7);
}
