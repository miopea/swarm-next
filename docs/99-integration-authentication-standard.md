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

### The registration's home tenant does NOT limit who can sign in. Measured.

The natural worry -- *"doesn't a registration in our tenant restrict this to our
own email domain?"* -- is the reason people register one app per install and make
setup harder than it needs to be. It is wrong, and the endpoint settles it:

```sh
# Azure CLI's client id, registered in MICROSOFT's tenant.
# /consumers is personal Microsoft accounts ONLY -- no organizations at all.
curl -sS -X POST https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode \
  -d "client_id=04b07795-8ddb-461a-bbee-02f9e1bf7b46&scope=openid"
```

Result, 2026-09-11: a `device_code` is issued. An app registered in one tenant
starts a **personal-account** sign-in without complaint. (Request the code only.
Do not complete the flow -- that is authenticating as somebody.)

Three things get conflated, and keeping them apart is the whole answer:

| | Whose | How many |
| --- | --- | --- |
| **App registration** | the publisher's | exactly one, ever |
| **The account signing in** | each user's own | one per Hive |
| **Service principal** | auto-created in the signer's tenant | one per organisation |

The registration is the identity of **the software**, not of a mailbox -- the
"published by" line. Who may use it is a separate field on it, **Supported
account types**, and *"any organizational directory and personal Microsoft
accounts"* is the one that admits everybody.

The real consequence of a shared registration is narrower than the feared one:
the consent screen carries the publisher's name, and a tenant that blocks user
consent to third-party apps makes its people ask an admin once. That is tenant
policy and it applies whether the app is yours or theirs.

### Where to put the registration -- a ONE-WAY DOOR

> *"Once created, you can't move the application object between different
> tenants."*
> -- [Register an application](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app)

Choosing the tenant is therefore not a detail to settle later. Moving it means a
NEW registration and a NEW client id, and every Hive in the field has to be
reconfigured, because the client id is what each install holds.

Weigh it on ownership rather than on who can sign in, which the section above
settles: the owning organisation appears as the publisher on every consent
screen, and its admins can rename, restrict or delete the app -- which would
break email for every install at once.

**A personal Microsoft account CAN hold one, through a directory.** There is no
registration without a tenant; signing in to Entra with a personal account
creates a free **Default Directory** and the app lives there. Microsoft's
prerequisite: *"A workforce or external tenant. You can use your Default
Directory for this quickstart."* All four audiences are offered, including
`AzureADandPersonalMicrosoftAccount`, and that audience's documented limits do
not bind this integration: at most two client secrets (we use none), a 90
-character display name, no national clouds, and at most 30 permissions per
resource against our three.

⚠️ **Not verified from the docs:** whether the portal's dropdown actually renders
the multitenant options inside an MSA-created Default Directory. Nothing says it
hides them, but that is an inference from absence. It is visible on the
registration form in seconds -- check there rather than trusting this line.

## Registering the app: four things that cost a session each

Learned building the Swarm Email registration on 2026-09-11. Every one of these
presents as a code bug and is a registration setting.

**1. A `web` redirect URI defeats a public client.** The platform an URI is
registered under decides whether client authentication is demanded. Redeeming
against a `web` URI with no secret fails `AADSTS7000218` -- *"The request body
must contain the following parameter: 'client_secret' or 'client_assertion'"* --
which reads as "our code forgot the secret" and is actually "this URI is the
wrong type". Register under **Mobile and desktop applications**. The Overview
blade states the tally plainly: `0 web, 0 spa, 2 public client`.

**2. An existing single-tenant app usually CANNOT be widened in place.** Setting
supported account types to include personal accounts fails with *"Property
api.requestedAccessTokenVersion is invalid"*. Personal accounts require v2
access tokens; an app created single-tenant is on v1. Fixable by editing the
manifest to `"requestedAccessTokenVersion": 2` first -- but only safe when the
app exposes no API of its own, so check the Application ID URI before doing it.
Creating a fresh registration with the right audience avoids the whole question.

**3. Every install's callback must be on the ONE registration, and the ceiling
is real.** Microsoft matches redirect URIs exactly. Wildcards are unsupported
once the audience includes personal accounts, query parameters are unsupported
too, and the cap drops from 256 to **100**. What makes a shared registration
workable anyway is the loopback rule: **the port is ignored when matching a
localhost URI**, so one `http://localhost/<path>` entry serves every port. Only
an install published at its own HTTPS address needs a line of its own. Do not
register two localhost URIs differing only by port -- the login server picks one
arbitrarily and applies its platform type.

**4. READ THE OVERVIEW, NOT THE FORM.** A changed dropdown that was never saved
looks identical to a saved one. The Overview blade renders the stored object:
supported account types, the redirect tally, and whether any credential exists.
It is the only screen that answers "what is actually true right now", and it is
what caught a supported-account-types change that had not been committed.

### Probing a registration from outside: what each endpoint can and cannot say

`POST /{tenant}/oauth2/v2.0/devicecode` judges the `client_id` with **no user
present**, which makes it the best available probe -- but read the error, since
two of them mean opposite things:

| Result | Means |
| --- | --- |
| `device_code` issued | app exists here, scopes fine, public client flows on |
| `AADSTS700016` not found in directory | the app is **not** available to that audience |
| `AADSTS70002` client not supported for this feature | the app **is** there; device code specifically is not offered |
| `AADSTS50059` no tenant-identifying information | you used `/common` or `/organizations`; device code needs a concrete tenant |

⚠️ **`/authorize` CANNOT be used to check a redirect URI.** It returns 200 and an
ordinary sign-in page for an unregistered client id *and* for a wrong redirect
URI, deferring both checks until after credentials. A control run with a
deliberately wrong redirect proved this: it was indistinguishable from the
correct one. **There is no way to verify redemption without a real sign-in** --
say so rather than implying the probe covered it.

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
