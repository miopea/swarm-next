# ADR 0081: Reassess a block without restarting work

Status: Accepted under the operator's September 6 dependency-cleanup direction.

Queen may correct the current reason and explicit not-before date of an already
Blocked local task through a dedicated application command. This is not a
Blocked-to-Blocked lifecycle transition, assignment, wake, briefing, dependency
edit, approval or resumption. Ordinary transitions remain unchanged.

The command requires the latest observed task activity sequence, a concise
current reason, and the evidence used to reassess it. A future not-before date
must represent a verified external window or existing operator instruction,
never a cooldown invented to hide routing debt. No date means the reason is
corrected but Queen still owns reassessment unless a real prerequisite or
pending decision determines another owner. Indefinite prose does not become a
machine-recognized hold merely by being rewritten.

Persistence checks local ownership, Queen/operator authority, Blocked state and
the observed sequence atomically. A saved identical retry is idempotent; a changed
or stale request conflicts. The latest concise reason is projected while the
original note and every reassessment remain in history. Reassessment is anchored
to its exact entry into Blocked; leaving/reentering Blocked cannot reuse an old
reason or date. No worker/session lifecycle or transport queue is touched.

Dependency cleanup still uses ADR 0076's verified explicit edges. Queen must read
the referenced tasks before linking them and follow the chain to movable work.
An elapsed date returns verification ownership to Queen, never auto-starts work.
This command supplies a missing repair capability, not proof that Queen used it
or that no-action review coverage is enforced. Those remain live acceptance work.
