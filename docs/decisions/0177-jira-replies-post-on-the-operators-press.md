# 0177 — Jira replies post on the operator's press

- Status: Accepted
- Date: 2026-11-19
- Decided by: the operator. They approved a reply to a support ticket and could
  not find it on the ticket; it arrived about two minutes later. Asked how far
  the fix should go, they chose "Post on my press": the worker drafts the
  reply, they edit it in Needs you, and their press posts it straight away. A
  worker's own comments are sent straight away too.
- Amends: ADR 0120 (a drafted reply's card stands in for the held
  resolution's card).

## Context

Two waits sat between the operator's approval and the ticket.

1. **The relay.** The worker asked the operator to approve its reply in a
   decision. The answer reached the worker on its next turn, about 70 seconds
   later in the case that prompted this. Only then did the worker call
   `swarm_comment_jira_task`.
2. **The queue.** That tool only queued the comment. Comments went out on the
   next Jira pass, which runs every 60 seconds: about 50 more seconds here. The
   operator's own comment from the task page was already sent at once.

## Decision

1. **The worker drafts the reply; it does not post it or ask about it.**
   - A new worker tool, `swarm_draft_jira_reply`, records the reply to whoever
     raised the ticket. It works on the worker's own Jira-linked task in any
     state, including completed. It posts nothing.
   - One draft per task. A later draft replaces an earlier one, but never a
     press still on its way to Jira: the worker is told to draft again once
     it lands. A later draft drops a press that Jira refused, or that Swarm
     lost track of, so the task's retry cannot send the old words as well.
   - The brief for a Jira-linked task names the tool, and says not to ask in a
     decision. `swarm_comment_jira_task` says the same in its description.
2. **Needs you shows the draft.** One card per ticket shows:
   - the issue, the project and the ticket's title;
   - who raised the ticket: its Jira reporter, which in a service desk is the
     customer. Each sync reads it. Until one has, the card says "the person
     who raised it";
   - the worker that drafted it;
   - the reply, in a box the operator can edit;
   - when the task is complete and its resolution waits on this reply, that
     Jira resolves the ticket once the reply is posted or dropped.

   The buttons are **Post reply**, **Don't post** and **Open the task**. When
   the ticket's resolution also waits on the operator (ADR 0120), **Post reply
   and resolve** comes first and is the primary button.

   A drafted reply is one Needs you item, counted and pushed. Its push uses the
   Jira resolution's kind: a push's kind changes nothing about how it is sent,
   and a new kind would rebuild the notifications table.
3. **One press posts the operator's words straight away.**
   - The press records their edit as what was posted. It queues the reply
     through the ordinary Jira comment delivery and sends it within the same
     request.
   - The card answers with what Jira did:
     - delivered;
     - refused, with the reason in words;
     - not sent yet, because Jira could not be reached. Swarm tries again by
       itself, up to three attempts;
     - uncertain, because Swarm stopped mid-send.
   - The card stays until Jira has the reply. A refused or uncertain reply can
     be posted again, and the new attempt replaces the failed one, so the
     task's retry cannot send it twice. While a reply is on its way, the
     buttons wait.
   - A delivered reply leaves Needs you. "Reply posted on …" stays in the page
     until the operator dismisses it. That result is kept in the page, not the
     database.
4. **A ticket never resolves ahead of its unposted reply.** The operator
   chose this when the review of this change found the gap.
   - On a project set to ask, Post reply and resolve releases the held
     resolution in the same transaction that records Jira taking the reply.
     A refused reply resolves nothing. A reply that lands on a retry resolves
     without another press.
   - On a project that does not ask, a completion still queues the ticket's
     resolution, but it is not sent while that ticket has a drafted reply
     that has not reached Jira. It goes once the reply lands, or once the
     operator chooses not to post it. Before this, the worker's task waited on
     the operator's decision, so it could not complete before the reply went.
     A draft has no such wait.
   - Other workflow moves are not held. Only the resolution is about the
     requester's answer.
   - The press and Don't post send a waiting resolution at once. The Jira
     pass now sends comments before transitions, so neither waits another pass
     and a ticket does not resolve before its last comment.
   - A press says the ticket resolved only when a resolution followed that
     reply.
5. **The held resolution's own card steps aside.** It is left out of the list
   and the count while a reply for that task waits. It comes back if the
   operator posts the reply without resolving it, or drops it.
6. **A worker's own comment is sent before the tool answers.**
   `swarm_comment_jira_task` sends straight away, as the operator's comment
   does. It tells the worker the resulting state, Jira's error if there is
   one, and what that means.
7. **Nothing posts without the press.** The worker's draft tool records a
   draft and nothing else.
8. **Schema 231** adds `jira_reply_drafts`, and `jira_reporter_name` on each
   linked issue. The worker tool surface moves to revision 44.

## Consequences

- An approved reply reaches the ticket within the press. In the tests that
  takes well under a second; a real Jira usually answers in about a second.
- The press waits for Jira. If the Jira pass is mid-send it waits for that too,
  since the two share one lock.
- An uncertain reply may already be on the ticket. Jira's comment call takes
  no key that would let Swarm tell, so the card says to check the ticket before
  posting again.
- A worker session that connected before the update cannot call the new tool
  until it reconnects. The revision is what says so.
- The task history line "Jira comment queued" still names the operator even
  when a worker queued the comment. This change leaves that alone.
- Not proven by the tests: a real post to Jira, which the tests replace with a
  fake. The first real press is that proof. Nor that every service desk
  project shows its reporter field to this Hive's account: one that hides it
  leaves the card saying "the person who raised it".
