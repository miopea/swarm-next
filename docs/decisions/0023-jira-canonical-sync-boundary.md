# ADR 0023: Jira canonical synchronization boundary

Status: **Accepted**

## Context

ADR 0010 makes Jira the canonical shared-work backend for the first Apiary
implementation. The product contract assigns external issue identity, mapped
workflow state, and human assignee to Jira while Swarm owns Hive and worker
assignment, execution state, local notes, evidence, and terminals.

A direct browser integration would expose credentials, make synchronization
depend on a mounted UI, and let refresh timing establish truth. Treating Jira
issues as ordinary local tasks would also lose remote identity and make imports
duplicate work. Jira outages, workflow customization, and concurrent edits need
explicit states rather than retries hidden in the Queen prompt.

## Decision

Jira is an adapter-private integration behind typed application commands.

- The API process owns the Jira client. Credentials are supplied by the local
  host, never persisted in Swarm, returned to the browser, or exposed to agents.
- Project discovery is read-only, bounded, paginated, and restricted to projects
  visible to the operator's Jira identity.
- A durable project binding records Jira's immutable project id and current key
  and name. A binding is Hive-owned or Apiary-owned; the initial local slice can
  create only Hive-owned bindings.
- Workflow mapping is explicit per Jira status id. Status category supplies a
  recommended Swarm state, but the operator must confirm the complete mapping
  before synchronization is Ready.
- Imported issues retain Jira's immutable issue id and current issue key. The
  issue id is the idempotency key, so repeated synchronization updates one
  linked task rather than creating duplicates.
- Jira owns the linked task's remote workflow state and human assignee. Swarm
  stores the last observed values and maps workflow state through the confirmed
  project mapping. Swarm-owned worker assignment, execution evidence, local
  notes, and terminal history are never overwritten by an import.
- Reads and writes are separate capabilities. The first vertical slice performs
  discovery, binding, mapping, and bounded read-only intake. Jira mutations use
  a later durable outbox with bounded attempts and explicit conflict state.
- Temporary network failure preserves already imported and owned work. It never
  authorizes a new Apiary claim. Invalid credentials, denied access, incomplete
  mappings, and network loss remain distinct typed readiness states.
- Queen receives typed project, issue, readiness, and command results through
  the application boundary. She never receives Jira credentials or permission
  to synthesize a successful transition.

## Consequences

The operator can use Jira work from the normal task and Queen workflows without
making the browser or Keeper a synchronization bottleneck. Duplicate imports,
silent status guesses, and credential leakage fail closed. Write synchronization
requires an additional outbox and conflict-resolution slice rather than being
smuggled into read-only intake.

## Validation

- Adapter tests cover secure URLs, pagination bounds, Jira response bounds,
  invalid credentials, denied access, malformed responses, and network loss.
- Persistence tests cover schema upgrade, project identity, complete workflow
  mapping, idempotent issue import, and preservation of Swarm-owned fields.
- API tests prove every endpoint is operator-private and no response contains
  credential material.
- Browser tests cover connected, degraded, unmapped, empty, and populated
  project states at desktop and mobile sizes.
