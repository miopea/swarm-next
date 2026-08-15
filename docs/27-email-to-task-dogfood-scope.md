# Email-to-task dogfood scope

Status: **Live dogfood intake implemented; multi-source reply fanout pending**

## Operator outcome

An operator can turn a real issue received by email into durable Hive work,
ship its solution, and close the loop with the sender without opening a second
task system or copying message content through the terminal. This is a frequent
intake path and belongs in the dogfood milestone, not the deferred integration
backlog.

## First useful slice

1. Link one operator-owned email integration.
2. Open **Bring in work** from the task board and choose **Email**.
3. Browse or search a bounded Inbox result set and select one message or up to
   20 related messages.
4. Import the selection atomically into one local task, including each readable
   body, inline image, and supported attachment as bounded task evidence.
5. Edit task planning fields and optionally choose a worker; leaving it
   unassigned routes it through Queen like other intake.
6. Keep every original message/thread identity and direct **Open email** link
   separately inside the merged task.
7. After the task reaches Completed *and* its approved deployment is recorded,
   prepare a plain-language resolution and reply to the original message.

Import is operator-initiated. It never scans the mailbox into task storage,
auto-creates work, or gives Queen access to mailbox credentials. Reply delivery
is a separate durable, idempotent outbox action gated by completion plus
deployment and the configured external-send policy.

The current durable reply outbox remains one delivery lifecycle per task. A
merged task therefore does not silently send the same resolution to every
source thread. Per-source fanout requires an additive target outbox with
per-thread idempotency, retry, uncertainty, and migration semantics before it
can be enabled safely.

## One-time Microsoft registration

Swarm uses a tenant-owned confidential Web application because the HTTPS
callback and authorization-code redemption run on the Hive host. PKCE protects
the authorization code and the confidential client secret remains required by
Microsoft for this server-side Web flow.

Settings exposes the exact callback URL and required delegated permissions
(`User.Read`, `Mail.Read`, and `Mail.Send`). The operator enters the tenant ID,
application ID, and newly created secret value once over the Hive's HTTPS
connection. Swarm writes them to a mode-`0600` file below its private state
directory, never returns the secret to the browser, and swaps the Outlook
adapter immediately without restarting the API or worker engine. Environment
configuration remains a host-admin override and cannot be replaced in the UI.

Microsoft protocol reference: <https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow>

## Privacy and ownership boundary

- OAuth tokens and Graph transport stay inside the Outlook adapter.
- Search results are bounded and fetched on demand.
- The preview distinguishes imported task evidence from source metadata.
- Sender, received time, message/conversation identity, and web link may be
  retained as typed source metadata.
- The readable message body and embedded issue images are included in the
  imported task by default. Active content, tracking pixels, remote image
  fetches, and unsafe HTML are removed; quoted history is visibly separated.
- Supported attachments are shown before import and retained under explicit
  count, per-file, aggregate-size, media-signature, and private-storage bounds.
- Imported content is untrusted task input, never instructions or automatic
  agent authorization.
- Stable source identity makes repeat imports idempotent or visibly links to
  the existing task instead of creating quiet duplicates.

## First-slice non-goals

- automatic inbox rules or background task creation;
- forwarding, moving, deleting, or marking mail read;
- synchronizing intermediate task state back into Outlook;
- ingesting an entire mailbox or attachment archive;
- using email participants as Swarm authorization identities.

## Follow-on decisions

The short implementation interview must settle:

1. how much quoted history belongs in the imported evidence;
2. the default Inbox time window and search behavior;
3. attachment types and limits needed for real issue reports;
4. whether a dogfood reply is always reviewed, may be auto-sent under a
   recorded policy, or varies by sender/domain;
5. whether a merged task replies to one selected source or every source, and
   the operator review required before fanout;
6. what constitutes deployment evidence for repositories without an automated
   deployment integration;
7. how shared mailboxes should appear after the first linked account works.

## Acceptance

- Desktop and Android-sized task-board flows can search, preview, edit, import,
  and reopen the source without horizontal overflow.
- Nothing is persisted before explicit import confirmation.
- Repeating the same import cannot silently duplicate work.
- Integration loss never blocks local tasks and is reported separately from
  authorization denial.
- Completion without deployment cannot enqueue a reply.
- The generated response is non-technical, references the reported issue, and
  is previewable before the configured send action.
- Reply identity is bound to the imported source; retries cannot send the same
  resolution twice.
- Tests prove content bounds, HTML sanitization, image/attachment rejection,
  import and send idempotency, credential isolation, lifecycle gating, and
  durable outbox recovery.
