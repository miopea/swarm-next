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

Still required: integration account-to-saved-profile orchestration. Microsoft
readiness includes name/address; Jira readiness currently exposes account_name
but no email. Do not mark the full identity request complete from feedback reuse.
Avoid choosing silently between conflicting connected identities, and preserve
saved operator choices. Keep private integration credentials out of this path.
