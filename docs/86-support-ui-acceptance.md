# Support UI acceptance checkpoint

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
