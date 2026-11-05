/**
 * The workspace's version, as the root Cargo.toml declares it.
 *
 * A capture from the harness shows a version in two places: the rail's
 * "Runtime" label, which reads /health, and the Updates card's current version.
 * Both were written into the fixtures by hand, so every capture said 1.0.0 or
 * 1.16.4 whatever the checkout was, and a README picture named a release it was
 * not taken from. `vite.harness.config.ts` reads Cargo.toml with this and hands
 * the result to the fixtures, so a capture names its own release.
 */
export function workspaceVersion(cargoToml: string): string {
  const section = cargoToml.split(/^\[/m).find((part) => part.startsWith("workspace.package]"));
  const version = section?.match(/^version = "(\d+\.\d+\.\d+)"$/m)?.[1];
  if (!version) throw new Error('Cargo.toml has no [workspace.package] version = "x.y.z"');
  return version;
}

/** The minor release after `version`, for a fixture that offers an update. */
export function nextMinor(version: string): string {
  const [major, minor] = version.split(".").map(Number);
  return `${major}.${minor + 1}.0`;
}
