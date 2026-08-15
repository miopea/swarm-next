const HANDOFF_PREFIX = "swarm-next-apiary";

export type ApiaryHandoffKind = "connection" | "invitation";

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
