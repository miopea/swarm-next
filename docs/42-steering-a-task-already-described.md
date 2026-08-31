# Steering a task already described

A recommendation for the operator, from a gap Queen hit repeatedly on
2026-08-31: her steering lives in transition notes, transition notes are not
delivered, and the channel that *is* delivered cannot change what the work is.

**The recommendation is to change nothing structural.** The argument is below,
including what it costs.

## The hinge question, answered from the code

Queen asked whether `operator_instruction` is unwritable by agents deliberately
or simply unbuilt. It is deliberate, and it is fully built.

The commit that introduced it (`7c61be0`) states the motivation in the first
sentence: *"The operator regularly needs to say something that governs how a
task is approached rather than what it contains — 'interview me first',
'analyse this, do not act on it'."* It is bounded to 280 bytes
(`MAX_OPERATOR_INSTRUCTION_BYTES`, `crates/swarm-persistence/src/lib.rs:119`)
**so that it cannot quietly become a second description competing with the
first**.

That commit closes with "backend only… the task form still needs somewhere to
type it", which is no longer true: `TaskDetailDialog` carries the field,
`TaskDetailsUpdate` carries it through the API, and `update_operator_task`
persists it. The operator can set it today. No agent tool can, which is
consistent with whose field it is rather than an oversight.

## What actually reaches a worker

| Channel | Written by | Delivered how |
| --- | --- | --- |
| `operator_instruction` | operator only | in the brief, before the work |
| operator rulings | operator, by resolving a decision | in the brief, newest first |
| description | whoever files the task, once | via `swarm_list_tasks` |
| amendments | any worker, including Queen | via `swarm_list_tasks`, with precedence stated |
| transition `note` | any worker | **nowhere** — history only |

Amendments are surfaced well, and that is worth stating because it was
uncertain: they arrive as their own top-level field with a note saying *"where
one contradicts the description, believe the amendment"*. This worker acted on
one today — Queen's correction of 49 to 31 changed what got measured, not just
what got read.

## Why the answer is "change nothing structural"

**The restriction is load-bearing, and `swarm_amend_task_facts` says why in its
own contract:** a worker that could move scope *"could redirect itself and then
be judged against a target it moved."* A Queen-writable redirect channel
reintroduces that hazard one level up. Work finished against the original
description could be invalidated by an instruction that arrived after it
started, and the record would show a worker missing a target it never had.

**Queen already has a full-strength delivered channel for changed work: a new
task.** A description is unlimited, arrives reliably, and is hers whenever she
creates one. What she cannot do is rewrite a description someone else wrote —
and that is the same protection, seen from the other side. The cost of the
alternative is one board row; the benefit is that a redirect is visible ON the
board rather than buried in a note only two transcripts hold.

**Today's incidents were not "Queen needed to redirect".** They were "Queen
believed notes were delivered". That cause is addressed on a separate branch
(`fix/transition-note-delivery`), where `swarm_transition_task`'s `note`
parameter says where it goes and names the channel that travels. If that branch
is not taken, this recommendation is weaker: the misconception that produced
today's incidents would still be live, and the case for a new channel would rest
on evidence gathered under it.
Building a new channel for the same incident would leave it untested against
real need — the need might evaporate now the misconception has.

**And `operator_instruction` in particular should stay the operator's.** A
worker reads it as the operator speaking. Letting Queen write it would put her
words in the operator's voice, which is the same shape as approving her own
exemptions: one actor on both sides of a check built for two. Queen has been
scrupulous about disclosing that when it happens; the fix is not to create more
of it.

## What this costs, stated rather than glossed

Queen's workaround is `SendMessage` to the worker's session. It works, and it is
off the board: the instruction exists in two transcripts and nowhere in the
record. That is a real loss and this recommendation does not remove it.

For a task Queen did not create — every worker-filed draft — her only recorded
channels are an amendment for facts and a new task for anything else. If the
right move is "narrow this task's scope", she must retire and refile, which
loses the assignment history.

## What would change the answer

Concrete cases the existing channels could not carry, collected over a week
rather than argued in advance. If they exist, the shape to build is a **separate
delivered steering note, attributed to Queen and never to the operator** — not
write access to `operator_instruction`. A channel that says who is speaking can
be weighed by the worker reading it; one that borrows the operator's voice
cannot.
