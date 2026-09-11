# Apiary-focused release candidate — September 11, 2026

Status: runtime candidate verified; release version, production release package,
signature, manifest publication and tag have NOT been created by this work.
This handoff does not authorize publishing a release or declare the full maturity
program complete. Source comparison: v1.7.1 through3dfc019e.

## Exact tested candidate

- Runtime source: `3dfc019e2d5e7a297063252c66ff69856c2e47e4`.
- Development build: `1.7.1-dev-3dfc019e2d5e-20260911103315-1290809`.
- CI34589715128: all four jobs passed, including full Rust and web suites.
- Production and WSL served the exact build and passed authenticated Edge checks.
- Production enginePID944143 and all ten exact worker/session pairs preserved.
- WSL engine explicitly updated afterward: PID43236, one loaded worker returned.
- Development bundle is NOT a signed public release artifact. Do not rename it
  into a release. Follow cutting-a-release.md once the operator chooses a version
  and authorizes publishing; verify bundled feedback credentials without exposing
  them. Confirm the final release source differs only by reviewed release metadata.

Detailed acceptance and limits:92-maturity-acceptance-ledger.md.

## Proposed What's New card

Title: **A more welcoming Apiary**

Join your team with less setup, see shared work more clearly, and keep each Hive's
local work its own.

1. **Join once.** Paste an invitation, review and submit; after Keeper approval,
   your Hive finishes joining automatically—even with the browser closed.
2. **Jira is optional.** Join and receive Swarm tasks without a Jira account.
   Connect only the Jira projects you actually use.
3. **One Apiary home.** Find invitations, membership, shared setup and next steps
   together on the Apiary page.
4. **Your Hive, recognized.** Reuse Jira or Microsoft profile details, review them
   before sharing, and reuse your saved identity for feedback.
5. **Easier worker setup.** Manage trusted project folders, discover repositories
   and use paths relative to your Hive's home directory.
6. **Shared work stays current.** Task lifecycle changes reach member boards;
   finished work stays available in optional history instead of crowding open work.
7. **Clearer waiting reasons.** Queues explain the last scoped terminal-delivery
   check and link relevant ownership and prerequisites.
8. **Lighter routine status.** Detailed Queen review data is requested on Queues,
   rather than carried through every ordinary status refresh.

## Curated fixes

- Restored scrolling in Apiary management and navigation to setup sections.
- Preserved reviewed identity when creating an Apiary.
- Recovered opening the Apiary view after membership was already saved.
- Made enrollment failures and retries visible; progress survives restarts.
- Kept optional Jira configuration separate from shared-work recovery problems.
- Corrected member-owned claim reads through Keeper.
- Preserved complete shared-task lifecycle states during synchronization.
- Refreshed shared-task boards through durable change notifications.
- Separated active shared work from retained completed history.
- Kept worker setup errors from breaking the form; made folder discovery retryable.
- Prevented failed provider checks from falsely reporting availability or no updates.
- Paused browser enrollment observation while hidden; joining continues server-side.

## Integration changes requiring explicit release review

The same source interval includes another worker's changes to Microsoft sign-in
and Jira authentication: bundled Microsoft registration, removal of per-Hive
registration, removal of Atlassian OAuth in favor of user API tokens, and Jira
token-age warnings. Source and test inspection confirmed these are intentional
breaking changes, not seamless credential migrations:

- Email commit fbe07bcb removes loading of per-Hive app registration and its write
  route. Bundled registration is used unless host environment pins one. The
  existing UI test supplies credentials_invalid and verifies a Reconnect
  Microsoft account action; it does NOT redeem an old real token or prove consent
  in a customer tenant. The removed registration route is covered by a405 test.
- Jira commit4e46e680 removes OAuth configuration/start/callback and bearer-token
  support. Startup loads existing saved API-token credentials, or complete
  host-provided site/email/API-token settings. Old OAuth-only configuration is
  ignored; it does not convert OAuth tokens into API tokens. The UI test verifies
  the required API-token form and its request, not a real-account migration.
- Both commits record their author's explanation that breaking/reconnecting
  existing configuration was discussed with the operator. That record is not a
  substitute for independent real-account acceptance.

These tests are included in successful candidate CI. No new external sign-in,
credential change, mail send or other-worker communication occurred in this
inspection. Real-account consent/reconnect remains a release-verifier check.

**Required upgrade notice:** If Outlook used your Hive's own Microsoft app,
reconnect your Microsoft account in Settings → Integrations after updating.
If Jira used Atlassian OAuth, connect it with your site address, account email
and a personal API token instead. Existing API-token Jira connections retain
that supported path. Jira remains optional for Apiary membership. An unavailable
integration does not require leaving and rejoining the Apiary.

## Closed live Apiary release gates

- Existing production/WSL membership upgraded in place; no leave/rejoin required.
- Directory and profile changes, shared task arrival/retirement and history checked.
- Fresh fictional Daisy joined the real HTTPS Keeper after one approval with
  no Jira configuration and no member browser open. On return:0/2 Jira projects,
  current synchronization, shared task cursor4/two applied tasks, correct directory.
- Test membership removed using the normal departure flow; the three original
  Hives remain. Disposable services stopped; audit and test data retained.

## Do not imply these are finished

Direct terminal answers automatically clearing Needs You; all Queen backlog and
failed-recovery behavior; broader Keeper remote-management capabilities; complete
Android/iOS attachment and handoff acceptance; automatic engine-update admission;
full fresh-versus-aged browser performance; BFG Admin's end-to-end support loop.
These stay on the full maturity plan and are not prerequisites silently removed
from that goal merely because this narrower release candidate is useful.
