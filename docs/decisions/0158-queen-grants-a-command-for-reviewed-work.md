# 0158 — Queen grants a command for reviewed work

- Status: Accepted; amended by ADR 0159 (Queen's cards are listed in her
  queue, not the operator's inbox, and a command card must say) and by
  ADR 0165 (Queen also grants guarded content writes and staging test
  sessions)
- Date: 2026-11-05
- Decided by: the operator, 2026-11-05. They said they had been pressing
  Allow on command cards without reading them, and that the ones they did
  read were obvious approvals: "How do we better automate these?" Offered
  delegation, fewer repeat cards, or both, they chose "Queen clears most
  cards": "Queen approves reviewed work, and you keep the short list above",
  the short list being "credentials/permissions, spend over a set amount and
  irreversible deletes". For the spend limit they chose "$25 (Recommended)".
- Amends: ADR 0129. The grant it describes can now also be minted by Queen,
  under an operator setting, and is recorded as hers. A third Hive setting
  sits beside "Queen releases reviewed work" (ADR 0131) and "Workers read
  production for their task" (ADR 0146), built the same way.

## Context

Measured read-only on the operator's Hive:

- **Over the seven days before the choice:** 220 command cards, about 30 a
  day. The operator had never denied one: 0 of 249 since September. 40 were
  allowed in bursts of three or more within a minute, which is reading
  nothing.
- **2026-11-01 to 2026-11-05:** 199 cards. 185 allowed, none answered any
  other way, 14 withdrawn by their workers. The median card waited 8.7
  minutes for its answer; one in ten waited more than 3.4 hours. A worker
  asking about its own Active task is Blocked for that wait (ADR 0119).

So the card had stopped being a check. It cost the operator a press each,
cost workers hours in the tail, and the operator's own account is that they
no longer read them. A check nobody reads is not one, and it trains the
reader to press Allow on the card that matters.

The operator kept four things for themselves. Everything else is work Queen
already judges: she reviews the task, and since ADR 0131 her review is
enough to release it.

## Decision

1. **A Hive setting, off by default: "Queen grants command cards".** Turned
   on and off in Settings with the operator's own credential, like ADR 0131's
   setting. A loopback request without it is refused, so a worker on the
   machine cannot send its own cards to Queen.

2. **The worker says which of the operator's reasons apply.** A command card
   (`swarm_request_decision` with `command`) gains `operator_only`, a list of
   the reasons that apply to the command:
   - `credentials`: it creates, sets, rotates, revokes or exposes a password,
     key, token or secret;
   - `permissions`: it grants, revokes or changes someone's access, a role,
     branch protection or a security setting;
   - `spend_over_25_usd`: it may cost more than $25;
   - `irreversible_delete`: it deletes something that cannot be restored.

   `[]` says none applies. Left out, the card says nothing, and it is the
   operator's. A name Swarm does not know is refused with the list, and the
   field is refused on a request that carries no command, so a wrong name is
   never read as "none".

   **Interpretation, open to the operator:** `credentials` is a change to, or
   exposure of, a credential. A command that authenticates by reading a
   secret from 1Password into its own environment, without printing it, is
   not a credential step. ADR 0146 already lets a worker do exactly that for
   a read. Reading the operator's "credentials" as every command that
   authenticates would send nearly every deploy and every probe back to them,
   which is the volume this decision exists to remove.

3. **A card is Queen's while the setting is on, it is pending, a worker
   other than Queen filed it, the worker said `[]`, and she has not handed
   it on.** That is one SQL predicate (`queen_card!`), read by everything
   that decides whose card it is:
   - the operator's Needs you and its push notifications leave it out;
   - a task's next-move owner does not read it as the operator's;
   - the workers list does not show its worker as awaiting the operator;
   - a task blocked on it says Queen answers, or the operator does;
   - it wakes Queen's conductor, so a review runs while one waits.

   A card Queen filed stays the operator's whatever it says, because she
   does not grant her own. Queen acts only on this Hive's cards, as the
   operator's Allow does.

   It is still in the operator's inbox, marked "With Queen", and they can
   answer it themselves. Turning the setting off makes every card theirs
   again at once, because the predicate reads the setting live.

4. **Queen grants or hands on, with three tools of her own.**
   `swarm_list_command_cards` lists the cards that are hers, in full.
   `swarm_grant_command` takes a note saying what she checked, and mints
   ADR 0129's grant unchanged: that exact text, once, in the session that
   asked, while the task is on the board and the script is unchanged. It then
   wakes the worker as the operator's Allow does.
   `swarm_hand_command_to_operator` takes a reason, puts the card in the
   operator's Needs you with that reason, and notifies them as a new card
   would. Queen cannot grant a card after handing it on. Both acting tools
   are Coordinate actions under her autonomy policy.

5. **Queen's grant is never the operator's ruling.** It is recorded with
   `granted_by_queen` naming her, resolution surface `queen_grant`, and no
   `resolved_by_operator_id`, so nothing that asks whether the operator
   answered reads it as theirs. Verifying it with `swarm_list_decisions`
   returns `verified: false` and `answered_how: granted_by_queen`, and says
   it is not an operator ruling. The worker's delivery reads "Queen granted
   this; the operator did not see it. Queen's note:". The worker brief says a
   decision marked `granted_by_queen` is not the operator.

6. **What Queen is told.** While the setting is on, her brief and each
   review's prompt say:
   - grant only work she has reviewed, and only what that work needs;
   - check the four reasons herself, whatever the worker said;
   - hand the card on when unsure.

   The review prompt also counts the cards waiting for her. A worker's brief
   says how to fill `operator_only` honestly, and to name a reason whenever
   it is unsure.

Queen still never runs a command. She grants a worker's command, as the
operator's button did.

## Threat model

- **A worker claims `[]` for a command a reason covers.** The card reaches
  Queen instead of the operator. Queen is told to judge the four reasons
  herself, and the worker's claim is not her review. The card stays visible
  in the operator's inbox. This is the residual risk the operator accepted
  by delegating, and it is bounded by ADR 0129: one exact command, once.
- **A worker grants a card.** `swarm_grant_command` requires Queen, checked
  in the application layer, and the store refuses any granter whose profile
  is not Queen's, so no other path can mint her grant. A worker calling it is
  refused and no grant is minted.
- **Queen grants her own card.** Refused: a card Queen filed is the
  operator's.
- **Queen grants a card that names a reason, says nothing, or was handed
  on.** Refused in the store, in the transaction that would grant, with what
  would make it hers.
- **The setting is turned on from the machine.** It needs the operator's
  credential, as ADR 0131's does.
- **Queen's grant is relayed as operator approval.** Verification says
  otherwise, and the delivery and brief name it as hers.

## Transition

- **Cards filed before this:** they carry no `operator_only`, so they are
  the operator's.
- **Workers already running:** they hold the older tool schema and cannot
  send `operator_only` until they restart. Their cards stay the operator's.
  The tool surface revision moved to 39, so the stale sessions are listed.
- **Queen already running:** she learns the setting from the next review's
  prompt; her new tools arrive when she reconnects.
- **The setting starts off.** Nothing changes until the operator turns it
  on.
- swarm-domain changed, so the worker engine build id changes and an engine
  update follows the merge.

## Rejected

- **Fewer repeat cards** (a plan approval covering its named steps, and no
  second card after an approval). The operator did not choose it. It stays
  possible on its own.
- **Queen grants every card.** The operator kept four kinds.
- **Swarm classifies the command itself.** A pattern over shell text is
  either too narrow to help or too broad to trust. The worker names the
  reasons, Queen checks them, and the operator can still see and answer
  every card.
- **Queen runs the command.** She coordinates; workers run commands.

## Not proven

- How many cards Queen will clear in practice. Today's cards carry no
  `operator_only`, so the count starts when workers restart onto this
  version and the operator turns the setting on.
- That Queen judges the four reasons well. Her grants are recorded as hers,
  so they can be audited.
- Live acceptance needs the merge, a reload, and the worker engine update.

## Evidence

### Tests

- The store: reasons are stored as one sorted list; a card with no reason is
  Queen's only while the setting is on; a named reason, none said, or a
  card Queen filed herself keeps it the operator's; Queen's grant runs once
  and is recorded as hers; a card she hands on is the operator's; a worker,
  its own or another's card, can neither grant nor hand on; a worker waiting
  on Queen is not listed as awaiting the operator; another Hive's card is
  neither listed for her nor grantable.
- The service: a worker is refused granting, handing on and listing before
  the store is asked; a worker's own Active task blocked on a card with Queen
  says it waits on Queen's answer, in its note and in the wait the board
  shows.
- Over the real MCP tools: an unknown reason and `operator_only` without a
  command are refused; with the setting off, Queen's grant is refused;
  turned on, the card is listed for her; a worker's grant is refused and
  mints nothing; Queen's grant mints the live grant; verifying it says
  `granted_by_queen` and not verified; a card naming `permissions` and a card
  she handed on are refused.
- The setting needs the operator's credential. Each brief carries its half
  only while the setting is on. The review prompt carries the guidance and
  the count. The worker's delivery names Queen's note.
- The inbox shows "With Queen", leaves the card out of the Needs you count,
  and counts it again once Queen hands it on. History labels Queen's grant.

### Ablations

Each guard was broken in turn, and the named test failed:

- Needs you keeps Queen's cards: the store tests.
- Queen may grant a card naming a reason: the store tests.
- Queen's grant verifies as the operator: the MCP test.
- An unknown reason is read as a known one: the MCP test.
- A worker may grant, with the service check removed: the service test.
  The first run of this ablation used the MCP test, where the worker
  granted its own card, and the store's own-card refusal hid the missing
  check. The service test now isolates it.
- The store lets anyone grant: the store test.
- Queen's note is labelled the operator's: the delivery test.
- The web counts Queen's cards in Needs you: the inbox test.

A read-only verification pass before the commit found four more places
that still read a card the wrong way: Queen's own card left the operator's
Needs you, the workers list and a blocked task's wait named the operator
for Queen's card, and her grant did not check the card was this Hive's.
Each now has a test, seen failing before its fix.

## Consequences

- With the setting on, the operator's command cards should drop to the four
  kinds they kept, plus whatever Queen hands them.
- A worker waiting on a card waits for Queen's next review rather than for
  the operator. The conductor wakes her when a card arrives.
- The auto-mode overlay, ADR 0129's grant mod and its spending are
  unchanged.
