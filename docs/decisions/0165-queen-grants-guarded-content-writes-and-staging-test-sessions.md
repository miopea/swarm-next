# 0165 — Queen grants guarded content writes and staging test sessions

- Status: Accepted
- Date: 2026-11-06
- Decided by: the operator, 2026-11-06. They asked for a review of the
  command cards they had approved, "to see if we can move more to the queen
  and also is the queen approving them as we planned or are they getting hung
  up?" The review offered two kinds of card to move. They answered: "Yes on 1
  and 2. Please do so".
- Amends: ADR 0158, decision 6 (what Queen is told) and the worker's half of
  decision 2. The four reasons, the store's rules and the `queen_card!`
  predicate are unchanged.

## Context

Measured read-only on the operator's Hive, over the 69 command cards filed in
the 28 hours from 2026-11-05 18:00 UTC:

- Queen granted 23, at a median of 5 minutes (8 at most). She handed 21 to the
  operator at a median of 3 minutes (11 at most), each with a reason naming
  one of the operator's rules. None waited on her, and none was open when the
  review was taken.
- The operator answered every handed-on card, at a median of 10 minutes and
  4.4 hours at most. All 66 answered cards were allowed, 43 by the operator
  and 23 by Queen, and none was denied.
- Queen handed nine cards on as production data writes or app settings. Five
  of those were display content: translation batches, UI strings and
  navigation entries. A sixth, a translation overwrite, went straight to the
  operator because its worker named `irreversible_delete`. Two of Queen's
  reasons cited an earlier ruling that the operator approves each translation
  batch themselves. Where her reasons say what she checked, she had read the
  script and named its guards: pinned by hash, refusing when the source or
  target rows had changed, and reading the result back.
- She handed two more on because the script minted a session through an
  application's sanctioned agent-session grant, for an allow-listed test
  fixture, to render pages read-only on staging. She counted the mint as
  creating a token.
- The rest were migrations, production app settings, production restarts,
  commands that run as the operator, and work beyond her brief. The cards
  workers sent straight to the operator were real deletions, permission and
  server changes, and a production slot swap. Three of those named the wrong
  reason (a settings change and a translation overwrite as
  `irreversible_delete`, a slot swap as `credentials`), but each would have
  reached the operator anyway.

So Queen is not where cards wait. The operator is, after she hands one on,
and these two kinds are cards she had already checked and could not grant.

## Decision

1. **Queen may grant a guarded production content write.** It changes only
   display content: translation or UI strings, or navigation entries. It never
   touches accounts, members' records, money, access, app settings or schema.
   She must find each guard in the script herself:
   - it is pinned by hash;
   - it reads every row it targets first, and refuses if any differs from
     what the card shows;
   - it records each value it overwrites and each row it adds, so the write
     can be undone;
   - it reads the rows back afterwards.

   Because the old values are kept, such a write deletes nothing that cannot
   be restored, so `irreversible_delete` does not apply. This replaces the
   operator's earlier rule that they approve each translation batch
   themselves.

   **Interpretation, open to the operator:** the review offered "small"
   guarded writes, and this sets no row limit. The cards it described ran from
   one cell to a few hundred strings. A count says nothing about whether a
   write can be undone, and the guards do. If the operator wants a limit, it
   is one sentence in Queen's guidance.

2. **Queen may grant a read-only staging check that signs in through an
   application's sanctioned agent-session grant.** The session is for a test
   fixture that the grant allow-lists. After the one mint, the script sends
   only GET and HEAD. It never touches production, a real member's account or
   an admin impersonation route, and it never prints the session or its code.
   That mint is not a `credentials` step: it creates a session the grant
   exists to create, not a credential anyone keeps.

3. **Everything else stays as ADR 0158 left it.** A guard Queen cannot find
   in the script makes the card the operator's. So do the production changes
   neither kind covers: migrations, production app settings, slot swaps,
   restarts and commands that run as the operator. Every other card she
   judges as before, by the four reasons. Her guidance says so in as many
   words, so the exception cannot read as taking those cards from her.

4. **What each side is told.**
   - Queen's grant guidance names both kinds, with their guards, says the
     per-batch rule is replaced, and lists what stays the operator's.
   - Her release guidance, which leaves production data to the operator,
     points at this exception rather than contradicting it.
   - A worker's grant guidance and the `operator_only` field description say
     neither kind is one of the reasons, so the worker sends `[]` and says in
     the card where the script keeps the old values or refuses everything but
     GET and HEAD.

## Threat model

- **A worker sends `[]` for a content write without the guards.** The card
  reaches Queen. She has to find every guard in the script, and a missing one
  makes it the operator's. This is ADR 0158's accepted risk, unchanged, and
  still bounded by ADR 0129: one exact command, once.
- **A script claims a guard it does not have.** Queen reads the script, not
  the claim, and the card pins the script's hash, so what she read is what
  runs.
- **The write turns out to be wrong.** The kept values are the undo, and the
  read-back shows what changed. The per-batch rule guarded against the same
  risk, and the operator allowed every batch it showed them.
- **The session check reaches production or a real member.** Neither kind
  covers that, so Queen hands it on. The sanctioned grant refuses fixtures it
  does not allow-list before any lookup.

## Transition

- Queen receives the new guidance in her next review's prompt, because a
  running Queen keeps the brief she connected with.
- Workers already running keep their brief and tool descriptions until they
  restart. Until then, a worker may still name `irreversible_delete` on a
  guarded write, and that card stays the operator's.
- Only swarm-api changed, so there is no worker engine update. The tool
  surface revision does not move: only a description changed, as in ADR 0159.

## Rejected

- **Swarm checks the guards itself.** ADR 0158 already rejected classifying
  shell text, and this is the same problem.
- **Moving production app settings to Queen.** They restart the app, and the
  operator was not asked about them.

## Not proven

- That Queen grants these two kinds in practice. The count starts when her
  next review carries the guidance.
- That workers stop marking a guarded write `irreversible_delete`. That
  depends on them restarting and on the guidance being clear.

## Evidence

- Unit test: Queen's grant guidance names both kinds and every guard, says
  the per-batch rule is replaced, and still hands on migrations, production
  app settings, slot swaps, restarts and commands that run as the operator.
  It also says every other card is judged as before. A first draft said
  "anything else" was the operator's, which would have taken from Queen the
  merges and releases she already grants; a read-only review caught it, and
  the test now pins the narrower wording.
  Her release guidance names this ADR. A worker's guidance says neither kind
  is `irreversible_delete` or `credentials`. The test was seen failing before
  the guidance changed.
- The existing tests still pass: each guidance is given only while its
  setting is on, and the release guidance still leaves production data,
  credentials, permissions, app settings, secrets, migrations and slot swaps
  to the operator.
