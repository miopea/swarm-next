# 0121 — An email reply can close without Swarm sending it

- Status: Accepted
- Date: 2026-10-23
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-23:
  - "Sometimes we don't need to send an email. So we should have a way to not
    send an email, and also an option to save it as a draft so it sits in my
    outlook."
  - Asked about the Outlook permission: "There is already a path to write to
    drafts in our linked mailbox. Verify that, but add both".
  - Asked whether a saved draft should leave Needs you: "Yes, it's in Outlook
    now".
- Builds on: the email-to-task slice (`docs/27-email-to-task-dogfood-scope.md`)
  and ADR 0120, whose held Jira resolutions also close on the operator's choice.

## Context

A completed email task sits in Needs you until a reply to it is delivered, and
the card's only way out is Send. Some threads need no answer. Others the
operator wants to finish in Outlook. Either way the item stayed in Needs you for
good. The only escape was removing the task, which retires the work instead of
saying the thread was handled.

The linked mailbox was checked when this was decided. Its token holds
`Mail.Read Mail.Send` and not `Mail.ReadWrite`, so it can read and send but not
write drafts. The legacy Swarm did write drafts, through `createReplyAll` with
`Mail.ReadWrite`. Whether today's app registration already has that consent
could not be told from the token, because Swarm never asked for it.

## Decision

### 1. Two new ways to close, both recorded

The card gains **Don't send** and **Save as draft in Outlook**.

- Each writes one `email_reply_outcomes` row (`not_sent` or `saved_as_draft`)
  and an activity note on the task.
- The row takes the task out of Needs you: the list, the badge and push all read
  the same filter.
- Neither sends anything.
- Don't send asks once before it closes, because the card does not come back.

### 2. A draft per person, from the mailbox the message arrived on

Save as draft calls Graph `createReply` with the reply as its comment, matching
`reply`, which a send uses. So the draft is addressed to the same person a send
would have reached.

- There is one draft per reply target, because an Outlook draft replies to one
  message.
- Each draft is recorded in `email_reply_outlook_drafts` as it is made. A
  failure part-way through is therefore retried from where it stopped instead
  of drafting the same person twice.
- A target already delivered is never drafted.
- A stale message id is looked up again by internet message id, as a send
  does.

### 3. Writing mail is asked for only when it is used

The stored token stays the read-and-send token every other call relies on.

- Saving a draft asks the refresh token for `offline_access Mail.ReadWrite` at
  that moment, and keeps any rotated refresh token.
- If Microsoft refuses the scope (AADSTS65001, AADSTS70000 or `invalid_scope`),
  the card says Outlook has not let Swarm save drafts and offers **Reconnect for
  drafts**. That sign-in asks for the usual scopes plus `Mail.ReadWrite`.
- The ordinary connect never asks for writing. A tenant that consents to
  reading and sending, but not writing, would otherwise stop the mailbox
  connecting at all.
- The ordinary refresh reports a refused scope exactly as it did before.

### 4. Nothing races a send

Closing and drafting both refuse while any target is queued, dispatching or
uncertain. A reply going out cannot truthfully be called "not sent", and
drafting it would answer the person twice.

A reply that has been closed is not then sent from the task page:
`queue_email_reply` refuses it.

## Consequences

- Schema 208 adds `email_reply_outcomes` and `email_reply_outlook_drafts`.
- Swarm now writes to the mailbox, but only drafts, and only when the operator
  presses the button. That amends the first-slice non-goal "synchronizing
  intermediate task state back into Outlook". Task state still never reaches
  the mailbox by itself.
- Swarm cannot see whether the operator sends a draft from Outlook. The task
  records that the reply was saved as a draft, not that it was sent.
- A draft created just before a crash, but not yet recorded, is made again on
  retry. The cost is a second draft in the Drafts folder; nothing is sent.
