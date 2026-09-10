import { fetchJiraReadiness } from "../api/jira";
import { fetchEmailReadiness } from "../api/email";

export type ConnectedIdentity = { source: string; name: string; email: string };

/** Suggestions only; reading accounts never saves or publishes a shared profile. */
export async function connectedIdentities(token: string, signal: AbortSignal): Promise<ConnectedIdentity[]> {
  const results = await Promise.allSettled([
    fetchJiraReadiness(token, signal), fetchEmailReadiness(token, signal),
  ]);
  const identities: ConnectedIdentity[] = [];
  results.forEach((result, index) => {
    if (result.status !== "fulfilled" || !result.value || result.value.connection !== "ready") return;
    const account = result.value;
    const name = typeof account.account_name === "string" ? account.account_name.trim() : "";
    const email = typeof account.account_address === "string" ? account.account_address.trim() : "";
    if ((!name && !email) || name.length > 120 || email.length > 254) return;
    const same = identities.find((identity) => identity.name === name && identity.email === email);
    const source = index === 0 ? "Jira" : "Microsoft";
    if (same) same.source += ` and ${source}`;
    else identities.push({ source, name, email });
  });
  return identities;
}
