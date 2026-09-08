# Support UI acceptance checkpoint

## September 8: live Admin-owned intake

The development Hive is healthy on
`1.6.0-dev-52623ab272da-20260908225740-1735005`; API PID 1736673 replaced
the prior API while terminal-host PID 1547164 and all 16 running sessions
remained. The first install was refused by the package lifecycle lock without
changing services. After confirming that owner had ended, one normal guarded
retry succeeded. No engine restart or release occurred.

The approved Admin origin is configured in the API-only systemd drop-in
`support-intake.conf`. No credentials or MCP workspace scope were added.
Rollback of intake configuration means removing that dedicated drop-in through
the normal managed API update; existing frozen reports must remain retained.

In a separate Edge tab at `https://swarm.bfgsolutions.net/?surface=decisions`:

- Opening Report a problem selected the configured Swarm Support form.
- Fictional contact `swarm-dogfood-20260908@example.invalid`, name Fictional
  Swarm Dogfood, and subject Fictional dev-Hive UI acceptance 2026-09-08 were
  reviewed before sending. Review still showed zero reports.
- Explicit send first showed Saved to Hive, not remote acceptance. Checking
  delivery status then showed Received by Swarm Support.
- Closing, fully reloading, and reopening retained exactly one confirmed
  report. Runtime status independently confirms sender running, one attempt,
  no refusal, and receipt below.

Submission `0ac43f48-c286-4df8-85e2-57d15cf2cbed` received conversation
`ebc282cc-3383-4a47-9ff3-5ec7662e67d7`, message
`71a869ab-d489-421d-8ed9-09e5d07195be`, timestamp 1788908443.
Admin-side read-only identity/content verification was requested from its
existing owner. This fixture requests neither a task nor an email send.

This proves live native submission/receipt/browser-reload retention, not an
interrupted live sender or mobile device suspension. Interrupted settlement and
exact replay are covered by the separately passed actual Admin-route paired
fixture. Native attachments, linked task completion and approval-to-reply
integration remain separate gates; the broader maturity goal is not complete.

## Historical local fixture checkpoint

September 7, 2026. Local integration only; no public support activation or release.

The no-proxy harness now exposes `support-feedback`, rendering the real dialog
against fictional in-memory reports. Desktop Edge exercised explicit review
without sending, saving as Pending rather than falsely claiming delivery, one
requested retry of an uncertain report, and confirmed-copy removal. After removal,
the pending report and uncertain original remained visible. No customer message,
Hive data, or central support service participated in these interactions.

Rendered review text exposed a readability defect: it inherited the tiny diagnostic
code-preview style. It now uses ordinary 16px message text, wrapping and bounded
scrolling. Visible desktop dialog buttons measured 44px high. The configured
390-by-844 iframe showed the narrow one-column form; this is responsive fixture
evidence, not native Android/iOS keyboard, picker or suspension acceptance.

The two support dialog test files pass all 10 tests; TypeScript checking and
`git diff --check` pass. Failure/recovery coverage remains in the domain,
persistence, transport, sender and UI suites, not in this visual fixture.

The live Hive remains healthy on `179d2df999de` without degraded subsystems or
database recovery required. Its separate Edge tab still shows the unlock screen.
Authenticated live UI acceptance remains open. This does not close the broader
maturity program, the central hosting decision, original-channel replies,
attachments, Admin registration, or linked task-completion integration.
