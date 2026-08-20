# ADR-0046: Preview-first Legacy migration with a reversible handoff

Status: Accepted

## Context

Swarm must eventually migrate existing Hives without asking Legacy Swarm to understand or write the Next database. Open local tasks are the first required slice. Jira remains canonical for Jira-linked work, while Legacy-only tasks, worker routing, learnings, decision history, and Routine candidates need an auditable path over time.

Writing both databases during one import would create a distributed transaction that SQLite cannot make atomic. A partial failure could hide work in Legacy before it is usable in Next. Treating every historical record as live Next state would also import obsolete behavior and noise.

## Decision

Swarm owns a versioned, bounded migration package and a preview-first import workflow.

1. The source Legacy snapshot is opened read-only and is never attached to the Next database.
2. Export produces a portable package with source identity, format version, source record IDs, and explicit exclusions.
3. Next recomputes validation, normalization, worker mapping, Jira exclusion, and duplicate detection for both preview and commit.
4. The operator explicitly selects records after reviewing transformed, skipped, invalid, unsupported, and duplicate results.
5. One SQLite transaction creates the selected Next records as Drafts, provenance links, and an immutable migration batch receipt. Source status and proposed Next state remain visible in the preview, but import cannot start workers, dispatch tasks, or trigger Queen automation.
6. Jira-linked tasks are skipped and later recreated by normal Jira synchronization.
7. A Legacy task that was Active is proposed as Ready because no Legacy provider process is transferred, but remains Draft until the operator approves normal work. Its prior state remains in provenance.
8. After the operator verifies the Next batch, a separate **Finish migration** step may create a fresh Legacy backup and apply the signed receipt. Legacy keeps the transferred tasks visible and read-only as `Moved to Swarm`; it does not mark them completed.
9. Finishing is never automatic. If an untouched Next batch is rolled back, its receipt can restore the corresponding Legacy tasks.
10. No dual write or ongoing synchronization exists between Legacy and Next.

The package envelope is extensible. Open non-Jira tasks and the durable worker
roster are independent review sections with separate commits and rollback
receipts. Worker import brings across the display name, repository path,
operator-reviewed routing description, provider choice, and relative ordering.
Every imported worker is sleeping and does not autostart. Provider processes,
terminal history, conversation content in the Swarm database or UI,
identity-file contents, groups, isolation settings, credentials, and approval
rules are excluded. For each
eligible Claude or Codex worker, preview may discover the latest exact provider
conversation identifier for that repository from the provider's local metadata.
The operator chooses whether to retain those identifiers; the option is explicit
and can be disabled to start every imported worker fresh. Retaining an identifier
does not wake a worker or copy terminal history. To make provider-native resume
truthful while Claude runs under an isolated Swarm profile, commit stages the
exact local Claude conversation file into that profile without parsing or
exposing its content. First wake repeats this check as a recovery path for
earlier imports and fails closed when the exact file is unavailable. Next then
asks the matching provider to resume that exact conversation. If no valid
matching identifier is available, preview says so and the worker starts fresh.
When the same repository worker already exists in
Next, the wizard offers a separate opt-in replacement choice. It requires the
worker to be sleeping, preserves the prior Next conversation for rollback, and
changes only the provider conversation identity. Names, descriptions,
providers, paths, tasks, and running workers are never overwritten. Existing
workers match by name or repository, and the managed Queen, Scout, and Legacy
Project Root identities are never duplicated. Any later section requires its
own normalization and review policy rather than inheriting task behavior.

## Consequences

Migration is resumable, explainable, and safe to rehearse. Duplicate imports can be rejected by source identity and record provenance. Legacy remains a recoverable reference during dogfooding without remaining the migration authority.

The workflow has deliberate confirmations and cannot promise one atomic transaction across both applications. Legacy finalization requires a compatible receipt consumer and backup support before it can ship. Attachments, dependencies, email reply identity, learnings, and historical messages remain unsupported until their own migration policies are implemented.

## Alternatives considered

- Let Legacy write the Next database: rejected because it couples the retired architecture to the replacement schema.
- Update Legacy during Next import: rejected because partial cross-database failure could hide live work.
- Leave imported tasks fully actionable in both applications: rejected because operators and Queens could perform the same work twice.
- Import every historical task: rejected because completed history would overwhelm the active queue and duplicate Jira.

## Validation

- Tests prove the exporter cannot mutate the Legacy snapshot.
- Preview and commit use the same normalization function and package digest.
- Import tests cover invalid records, Jira exclusions, exact worker matching, active-to-ready transformation, duplicates, transaction rollback, and provenance.
- Rollback rejects batches whose imported tasks changed after import.
- Browser tests prove that no record is imported before explicit selection and confirmation.
- Legacy finalization tests use a disposable snapshot, verify its pre-write backup, and prove receipt reversal before the feature is exposed.
- Worker tests prove managed-identity and duplicate exclusions, sleeping import,
  stable ordering, optional exact provider-conversation resume, fresh-session
  fallback, provenance, historical-schema upgrades, and rollback refusal after
  a worker is edited, awakened, or assigned work.
