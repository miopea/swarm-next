const HANDOFF_PREFIX = "swarm-next-apiary";
const stagedHandoffs = new Map<ApiaryHandoffKind, string>();

export type ApiaryHandoffKind = "connection" | "invitation" | "keeper";

function encodeBase64Url(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = "";
  bytes.forEach((byte) => { binary += String.fromCharCode(byte); });
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

function decodeBase64Url(value: string): string {
  const padded = value.replaceAll("-", "+").replaceAll("_", "/").padEnd(Math.ceil(value.length / 4) * 4, "=");
  const binary = atob(padded);
  return new TextDecoder().decode(Uint8Array.from(binary, (character) => character.charCodeAt(0)));
}

export function createApiaryHandoffLink(kind: ApiaryHandoffKind, payload: unknown, origin = window.location.origin): string {
  const url = new URL(origin);
  url.pathname = "/";
  url.search = "";
  url.hash = `${HANDOFF_PREFIX}-${kind}=${encodeBase64Url(JSON.stringify(payload))}`;
  return url.toString();
}

export function readApiaryHandoffLink<T>(value: string, expectedKind: ApiaryHandoffKind): T {
  const trimmed = value.trim();
  if (trimmed.length > 768 * 1024) throw new Error("That Apiary link is unexpectedly large.");
  let fragment: string;
  try {
    fragment = new URL(trimmed).hash.slice(1);
  } catch {
    throw new Error("Paste the complete Swarm Apiary link.");
  }
  const prefix = `${HANDOFF_PREFIX}-${expectedKind}=`;
  if (!fragment.startsWith(prefix)) throw new Error(`That is not a Swarm Apiary ${expectedKind} link.`);
  try {
    return JSON.parse(decodeBase64Url(fragment.slice(prefix.length))) as T;
  } catch {
    throw new Error("That Swarm Apiary link is damaged or incomplete.");
  }
}

export function currentApiaryHandoffLink(kind: ApiaryHandoffKind, location = window.location): string | undefined {
  const prefix = `#${HANDOFF_PREFIX}-${kind}=`;
  return location.hash.startsWith(prefix) ? location.href : undefined;
}

export function retargetApiaryHandoffLink(value: string, destination: string, kind: ApiaryHandoffKind): string {
  readApiaryHandoffLink(value, kind);
  let target: URL;
  try {
    target = new URL(destination.trim());
  } catch {
    throw new Error("Enter the complete address of your personal Hive.");
  }
  const localHttp = target.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(target.hostname);
  if ((target.protocol !== "https:" && !localHttp) || target.username || target.password) {
    throw new Error("Use an HTTPS personal Hive address, or localhost for local development.");
  }
  target.pathname = "/";
  target.search = "";
  target.hash = new URL(value).hash;
  return target.toString();
}

export function stageApiaryHandoff(kind: ApiaryHandoffKind, link: string): void {
  readApiaryHandoffLink(link, kind);
  stagedHandoffs.set(kind, link);
}

export function peekStagedApiaryHandoff(kind: ApiaryHandoffKind): string | undefined {
  return stagedHandoffs.get(kind);
}

export function clearStagedApiaryHandoff(kind: ApiaryHandoffKind): void {
  stagedHandoffs.delete(kind);
}

export function takeStagedApiaryHandoff(kind: ApiaryHandoffKind): string | undefined {
  const link = stagedHandoffs.get(kind);
  stagedHandoffs.delete(kind);
  return link;
}
