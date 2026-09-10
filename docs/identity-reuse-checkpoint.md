# Operator identity reuse — September 10

Approved outcome: take name/email from connected Jira or Microsoft before asking
the operator, ask only for missing details, preserve explicit saved edits, and
reuse that saved identity for feedback. Show what joining shares; never infer an
email from a display name. This does not require leaving/rejoining an Apiary.

Completed local slice: SupportFeedbackDialog reads the existing saved public
profile once with a five-second abort deadline and unmount cancellation. It fills
contact fields without blocking composition or overwriting edited/cleared fields.
The placeholder Operator is not a real name. Prefill alone is not a dirty draft.
Per-message edits do not silently change the shared profile. Previously retained
reports keep their exact identity and payload when retrying.

Validation: 18 focused feedback tests passed, then a fifth identity regression
proved retained-report preservation (all five identity tests pass). TypeScript
checking passed. No actual report was sent. Live Edge visual acceptance remains
open because the testing runtime fails before connecting. Not deployed here.

Published feedback commit `e6d1f39a` subsequently passed all four CI jobs in
run34533407331. This covers the saved-profile feedback change, not the uncommitted
integration-import or native-answer server code. Deployment remains unverified.

Next local slice now implemented but not published: the profile form immediately
shows saved details, then reads Jira/Microsoft readiness only when information is
missing. A single unambiguous account supplies missing fields; distinct accounts
offer an explicit choice. Saved complete profiles avoid the extra requests. Late
responses cannot overwrite edits. Lookup deadlines/unmount cancellation are
bounded; unavailable integrations leave manual entry usable. Joining saves the
reviewed profile through the existing endpoint and existing default Hive naming.

The Jira adapter now includes its available profile email, or the authenticated
API-token login email after successful readiness. OAuth-hidden email remains
missing, never guessed. The operator approved isolated source transfer. All13
Jira adapter tests, including the two new identity regressions, and strict
API/persistence Clippy pass in `/tmp/swarm-native-answer-check.6fkv8E`.
No integration credential enters the UI. This identity slice changes neither
the database schema nor the worker-engine protocol.

The profile UI passes33 focused onboarding tests and TypeScript checking. Full
Jira/Microsoft integration acceptance still requires the live browser journey;
do not substitute these automated checks for that acceptance.
