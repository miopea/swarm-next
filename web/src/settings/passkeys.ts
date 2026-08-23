import { authenticatedFetch } from "../api/request";

export type RegisteredPasskey = {
  credential_id: string;
  relying_party: string;
  label: string;
  created_at: number;
  last_used_at: number | null;
  /** A credential registered at another address cannot sign in here. */
  usable_here: boolean;
};

/**
 * WebAuthn speaks in ArrayBuffers and the API speaks base64url, so every
 * boundary crossing converts. Doing it in one place keeps the conversions out
 * of the components, where a missed one produces an opaque browser error.
 */
function fromBase64Url(value: string): Uint8Array {
  const padded = value.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded.padEnd(padded.length + ((4 - (padded.length % 4)) % 4), "="));
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

function toBase64Url(value: ArrayBuffer): string {
  const bytes = new Uint8Array(value);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

type CreationOptions = { publicKey: PublicKeyCredentialCreationOptions & { challenge: string; user: { id: string } } };
type RequestOptions = { publicKey: PublicKeyCredentialRequestOptions & { challenge: string } };

export function passkeysSupported(): boolean {
  return typeof window !== "undefined" && "PublicKeyCredential" in window;
}

export async function listPasskeys(operatorToken: string): Promise<RegisteredPasskey[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/auth/passkeys");
  return response.json() as Promise<RegisteredPasskey[]>;
}

export async function removePasskey(operatorToken: string, credentialId: string): Promise<void> {
  await authenticatedFetch(operatorToken, `/api/v1/auth/passkeys/${encodeURIComponent(credentialId)}`, {
    method: "DELETE",
  });
}

export async function registerPasskey(operatorToken: string, label: string): Promise<void> {
  const started = await authenticatedFetch(operatorToken, "/api/v1/auth/passkeys/register/start", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ label }),
  });
  const { challenge_id, options } = (await started.json()) as { challenge_id: string; options: CreationOptions };

  const publicKey = {
    ...options.publicKey,
    challenge: fromBase64Url(options.publicKey.challenge),
    user: { ...options.publicKey.user, id: fromBase64Url(options.publicKey.user.id) },
    excludeCredentials: options.publicKey.excludeCredentials?.map((credential) => ({
      ...credential,
      id: fromBase64Url(credential.id as unknown as string),
    })),
  } as PublicKeyCredentialCreationOptions;

  const created = (await navigator.credentials.create({ publicKey })) as PublicKeyCredential | null;
  if (!created) throw new Error("This device did not create a passkey.");
  const attestation = created.response as AuthenticatorAttestationResponse;

  await authenticatedFetch(operatorToken, "/api/v1/auth/passkeys/register/finish", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      challenge_id,
      label,
      credential: {
        id: created.id,
        rawId: toBase64Url(created.rawId),
        type: created.type,
        response: {
          clientDataJSON: toBase64Url(attestation.clientDataJSON),
          attestationObject: toBase64Url(attestation.attestationObject),
        },
      },
    }),
  });
}

/**
 * Signs in with a passkey. Unauthenticated by design — this is the door, and it
 * mints the same session cookie a token does.
 */
export async function signInWithPasskey(): Promise<void> {
  const started = await fetch("/api/v1/auth/passkeys/authenticate/start", {
    method: "POST",
    credentials: "same-origin",
    cache: "no-store",
  });
  if (!started.ok) {
    throw new Error(
      started.status === 404
        ? "No passkey is registered for this address yet."
        : "A passkey sign-in could not be started.",
    );
  }
  const { challenge_id, options } = (await started.json()) as { challenge_id: string; options: RequestOptions };

  const publicKey = {
    ...options.publicKey,
    challenge: fromBase64Url(options.publicKey.challenge),
    allowCredentials: options.publicKey.allowCredentials?.map((credential) => ({
      ...credential,
      id: fromBase64Url(credential.id as unknown as string),
    })),
  } as PublicKeyCredentialRequestOptions;

  const asserted = (await navigator.credentials.get({ publicKey })) as PublicKeyCredential | null;
  if (!asserted) throw new Error("No passkey was offered.");
  const assertion = asserted.response as AuthenticatorAssertionResponse;

  const finished = await fetch("/api/v1/auth/passkeys/authenticate/finish", {
    method: "POST",
    credentials: "same-origin",
    cache: "no-store",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      challenge_id,
      credential: {
        id: asserted.id,
        rawId: toBase64Url(asserted.rawId),
        type: asserted.type,
        response: {
          clientDataJSON: toBase64Url(assertion.clientDataJSON),
          authenticatorData: toBase64Url(assertion.authenticatorData),
          signature: toBase64Url(assertion.signature),
          userHandle: assertion.userHandle ? toBase64Url(assertion.userHandle) : null,
        },
      },
    }),
  });
  if (!finished.ok) throw new Error("That passkey was not accepted.");
}
