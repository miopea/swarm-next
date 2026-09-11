# How an integration authenticates, and what each provider actually allows

The operator, on why Jira and Email each want a token *and* a sign-in: *"Why do
we need both when we're staying in a user context most of the time?"*

The short answer is that "both" is usually one credential wearing two fields, or
two credentials answering different questions — and where it is genuinely two,
one of them can often be removed. This is what each provider permits, **measured
rather than read**, because the published documentation disagreed with itself on
the one that mattered.

## The standard

**A new integration is a public client using PKCE, with no client secret.**

A confidential-client secret assumes the application can keep it. A self-hosted
Hive cannot: the secret is configured into, or shipped with, every install, so it
is not meaningfully secret. PKCE exists precisely for that case — the client
proves possession of a one-time verifier instead of a shared secret.

`mcp_oauth.rs` already states the principle: *"PKCE is required, not offered. A
public client without it is …"*.

**Where a provider refuses a public client, prefer the user's own token over an
app secret.** A user-held API token is scoped to that person, revocable by them,
and ships nothing to every install. A confidential OAuth client would add a
secret that reaches every Hive and can be extracted with `strings`.

**Two authentications are not redundant when they answer different questions.**
A client secret authenticates *the application* to the provider. Consent
authenticates *the user* to the application. Removing the first is a real
simplification; removing the second would make a Hive act as a service account
rather than as a person, which is not what anyone wants.

## Atlassian — a public client is NOT possible. Measured.

⚠️ **Do not re-derive this from the documentation. The documentation is
ambiguous and the tickets are easy to misread.** Probe the live endpoint:

```sh
# Same deliberately-invalid code both times. One variable changes.
curl -sS -X POST https://auth.atlassian.com/oauth/token \
  -H 'Content-Type: application/json' \
  -d '{"grant_type":"authorization_code","client_id":"<id>",
       "code":"deliberately-invalid-code",
       "redirect_uri":"http://localhost/callback","code_verifier":"<verifier>"}'
```

Result, 2026-09-10:

    PKCE verifier, NO client_secret   -> "Incorrect request parameters"
    PKCE verifier, WITH client_secret -> "authorization_code is invalid"

With the secret Atlassian got **past client authentication** and went on to judge
the code, rejecting it because it was invented. Without the secret it never
reached the code. The secret is what authenticates the client; PKCE alone does
not. Put the secret in a file and pass `-d @file` — never in `argv`.

Two traps for whoever revisits this:

- **`OAUTH20-2491` is Data Center, not Cloud.** Its workaround is the startup
  property `-Datlassian.oauth2.provider.validate.client.secret=false`, which only
  a self-hosted instance has. Citing it for Cloud is a mistake; it was made here
  first.
- **The Cloud ticket is [`ECO-283`](https://jira.atlassian.com/browse/ECO-283)** —
  "Gathering Interest", unresolved, last updated 27 Jul 2026. Filed May 2024, so
  check the *last update* rather than the age.
- **"Jira Cloud for Outlook signs in with no secret" is not counter-evidence.**
  Microsoft mandated Nested App Authentication for Office add-ins; the Office host
  brokers the token. That is a Microsoft mechanism, not Atlassian accepting a
  public client.

### So Jira uses the user's own API token

Site address, account email, and a token the user creates. Both **scoped** and
**classic** tokens work with basic auth; use **classic**. Atlassian's own
guidance: *"if you need to use an app that does not currently support API token
with scopes, you can create a token without scopes."* Choosing scopes means
guessing a set, and a wrong guess fails at some later API call rather than at
connect time.

⚠️ **Atlassian API tokens expire after one year by default** (tokens created
after 15 Dec 2024). A Hive's Jira connection therefore dies at the twelve-month
mark and presents as sudden auth failures long after anyone remembers setting it
up. Record the expiry and warn before it, or this becomes a support burden that
looks like a bug.

## Microsoft — a public client IS possible, for personal and corporate alike

Microsoft's protocol reference is explicit:

> **Public clients, which include native applications and single page apps, must
> not use secrets or certificates when redeeming an authorization code.**

`microsoft_oauth.rs` already performs PKCE correctly (S256 challenge, verifier at
redemption, pinned by a test). Only the `client_secret` in the token request
makes it a confidential client.

**Use `/common`.** It serves "any organizational directory **and personal
Microsoft accounts**". `/organizations` is organizations only and silently
excludes every personal account — an easy and invisible mistake.

⚠️ **CORRECTION, 2026-09-11.** An earlier version of this file said `common` was
*"rejected by a string check"*. That was wrong, and the mistake is worth keeping
because of its shape: the validation allows any ASCII alphanumeric-or-hyphen
string, so `common` and `consumers` have always passed — only the **error
message** named "a tenant UUID or organizations", and the message was read as
though it were the rule. The values that make personal accounts work looked
unsupported while being accepted the whole time.

Now pinned by `every_authority_that_serves_personal_accounts_is_accepted`, and
the message names all four forms.

**Register the redirect as `native`, not `spa`.** Refresh tokens for native apps
have no specified lifetime; for an `spa` redirect they expire after 24 hours and
force interactive re-authentication daily. On an unattended Hive that failure is
invisible for a day and then looks like a bug.

The scopes this integration requests are all user-consentable and all support
personal accounts:

| scope | personal MSA | admin consent |
| --- | --- | --- |
| `User.Read` | supported | no |
| `Mail.Read` | supported | no |
| `Mail.Send` | supported | no |

So the only remaining consent barrier is **tenant policy, not our permissions**:
a tenant administrator can disable user consent to third-party apps, and where
that is set, admin consent is always required. Say *"ask your admin to approve
this once"* and offer a `prompt=consent` link — never a raw error. The person
connecting a mailbox is usually not an admin.

Multitenant also requires a globally unique App ID URI and token validation that
accepts multiple issuer values.

### An app registration cannot be created by signing in. Measured.

The obvious hope -- *"shouldn't the first authorization from an admin create
it?"* -- is worth killing with evidence, because the answer sounds like it
could go either way and the flows are easy to confuse.

The device-code endpoint decides on the `client_id` alone, with no user present,
which makes it the clean probe:

```sh
curl -sS -X POST https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode \
  -d "client_id=11112222-3333-4444-5555-666677778888&scope=openid"
```

Result, 2026-09-11:

    unauthorized_client
    AADSTS700016: Application with identifier '1111...8888' was not found in the
    directory '9188040d-6c67-4c5b-b112-36a304b66dad'

⚠️ **Do not probe this at `/authorize` instead.** That endpoint returns **200 and
a normal sign-in page** for an unregistered client id -- it defers the
app-existence check until after credentials, so the request that proves the
point looks like the request that worked.

**What admin consent DOES create automatically is the service principal** -- the
"Enterprise application" object in the consenting tenant -- for an app
registration that already exists somewhere. That half needs no work from anyone.
The registration itself is a prerequisite: one of them, in one directory, serves
every tenant that later consents.

## Providers deliberately not supported

Recorded so they are not reopened without the reasons. Operator: *"This is a
free dev app."*

**Apple.** No public OAuth path for iCloud Mail. Sign in with Apple is identity
only — a name and an email address, not mailbox access; CloudKit is app data. An
Apple Developer account does not unlock it, and the developer-forum thread asking
exactly this has no definitive answer. The realistic route is IMAP/SMTP with an
app-specific password: a second transport and a different credential story, not
an entry in a provider menu. Held as *no public path found* rather than
*impossible* — Developer Support could say otherwise.

**Google.** Technically the best fit — PKCE supported, `client_secret` explicitly
optional, loopback `http://127.0.0.1:port` the recommended desktop redirect
(custom URI schemes and copy/paste OOB are both dead). The cost is the blocker:
`gmail.send` is merely *sensitive*, but `gmail.readonly` is **restricted**, and
*"if you store restricted scope data on servers (or transmit), then you must go
through a security assessment"* — annually, by a third party. Swarm reads and
stores mail server-side, so reading Gmail is a funded compliance commitment
renewed every year. Send-only would be far cheaper, and that split is the
decision to revisit if Google is ever wanted.

## What to do for the next integration

1. Try a public client with PKCE and no secret.
2. **Probe the token endpoint before believing the documentation.** Send a
   deliberately-invalid code with and without the secret and compare which
   validation fails first. Two minutes settled what four documents disagreed
   about.
3. If the provider requires a confidential client, prefer the user's own token
   and record why.
4. If a shipped app secret is genuinely the only route, that is an operator
   decision with its blast radius stated — the precedent is the bundled feedback
   credential, and an extracted OAuth secret is a different risk from an
   extracted issues-write token: impersonation on a consent screen rather than
   direct access.
