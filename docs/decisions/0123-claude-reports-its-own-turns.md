# 0123 — Claude reports its own turns, and Swarm reconnects its tools

- Status: Accepted
- Date: 2026-10-23
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-23:
  - Shown the options with what each would do: "Build yoru recommendations of
    1 and 2. These are huge upgrades, especially the state detection." Item 1
    was the turn hooks; item 2 was `/mcp reconnect all`, together with the
    tool-list notice it shares the relay with.
  - Mid-build: "also keep in mind how this works when we have a shell running
    wihich we just added for state inspection".
- Builds on: ADR 0122 (Claude workers update in place), ADR 0053 (a session
  holding an older tool list), ADR 0086 (bounded MCP discovery recovery), and
  the screen classifier behind "Worker activity and attention" in the
  capability inventory.

## Context

Swarm decides whether a Claude worker is Resting, Buzzing or waiting on the
operator by reading its screen. That reader is 1,188 lines and has needed eight
fixes: a question read as busy, auto mode read as idle while it worked, a
footer cut off at narrow widths. Any change to Claude's interface can break it
without a sound. Delivery, the coordinator, the stale-work flag, the engine
update's mid-turn count and ADR 0122's `/restart` all depend on that reading.

Claude Code has hooks that report the turn itself. Measured in throwaway
sessions of 2.1.310 on 2026-10-23, not taken from the docs:

- Every typed prompt fires `UserPromptSubmit` and every finished turn fires
  `Stop`. A turn Claude starts by itself, such as the one after a background
  shell finishes, fires both.
- `Stop` lists the background tasks still running, by kind. When one finishes,
  Claude starts a short turn whose `Stop` lists the rest; a killed shell does
  the same.
- A message typed while Claude streams fires `UserPromptSubmit` at once, under
  the running prompt's id. If it is queued, the turn that answers it ends with
  a `Stop` and no `UserPromptSubmit` of its own.
- **Esc and Ctrl+C fire no hook at all.** Claude prints
  `⎿  Interrupted · What should Claude do instead?` over its prompt instead.
  A finished turn leaves `✻ Baked for 3s · done 10:14 PM` there (the verb
  varies; this Hive's terminal history shows a dozen).
- A local command such as `/mcp reconnect all` is not a turn: no hook fires.
- The docs add `StopFailure` with the error that ended a turn, `Notification`
  types for a waiting permission dialog and for a usage limit Claude will not
  resume, and `PermissionDenied` for an auto-mode refusal.

Separately, workers reach Swarm's tools through `swarm-terminal-host
mcp-proxy`, a stdio relay. Measured: when its first connection fails, Claude
closes the relay at once and never retries it. `/mcp reconnect all` starts a
fresh one in under a second, prints `⎿  Reconnected 1 of 1 MCP server`, and
gives the prompt back empty. And Claude lists a server's tools again as soon
as the server sends `notifications/tools/list_changed`, even while idle.

## Decision

### 1. The hooks are installed beside the existing ones

The host's startup overlay adds `UserPromptSubmit`, `Stop`, `StopFailure`,
`PermissionDenied`, and `Notification` matched to `permission_prompt`,
`elicitation_dialog`, `quota_auto_resume_stale` and
`quota_auto_resume_disabled`. Each runs `swarm-terminal-host provider-turn`.
The interview hooks it already installs (`PreToolUse`/`PostToolUse` on
AskUserQuestion, `PostToolBatch`) also feed the turn: an open AskUserQuestion
is a question waiting.

The helper reads up to 8 MiB, because `Stop` carries the whole final answer
and `UserPromptSubmit` the whole prompt, and forwards only a compact form of
the event: never prompt or message text, never a refused tool's input. It
prints nothing, ever, because what a `UserPromptSubmit` hook prints is added
to the prompt.

### 2. No protocol change

The compact report travels in the existing `provider_interview` host request,
authenticated by the provider incarnation's capability, as every Claude hook
except session start and end already does. Renaming the request would be a
protocol change and a protocol migration for every Hive. The host applies a
turn report and passes the interview hooks on to capture unchanged.

The host keeps one `ProviderTurn` per session: phase (Working, Ended, Failed),
the failure, what is waiting, what the last finished turn left running, and
the latest refusal. It accepts a report only from the live provider process,
and only about the current conversation once the startup gate has selected
one. Every terminal read now carries the turn and the terminal's last output
time, so a reader sees the screen and the turn from one moment.

### 3. How the API combines them

The hooks are exact about what they report and silent about the rest, so
neither source replaces the other:

| Screen | Turn | Reads as |
| --- | --- | --- |
| A question or permission menu | anything | Awaiting operator |
| Resting, with Claude's end-of-turn or interrupt line directly over the prompt | Working | Resting |
| Resting, terminal silent for 60 seconds | Working | Resting |
| Working | anything with a wait reported | Working |
| Not working | Working, with a wait reported | Awaiting operator |
| Anything else | Working | Working |
| Not working | Ended or Failed, a usage limit Claude will not resume | Awaiting operator |
| Anything | Ended or Failed | What the screen says |

The second and third rows are what keep a worker from being pinned busy: an
interrupt fires no hook, and a `Stop` can be lost. The line over the prompt
answers both at once. The quiet minute covers the case where both the hook and
the line are missing, for example after a Claude release draws the line
differently. It is safe because a working Claude redraws its spinner and
elapsed time every second, and it is never the first answer. The last row is
deliberate: a queued message's turn and `/compact` produce no hook, and the
screen is right about them.

Background work comes from the last `Stop`'s list when the latest turn
**ended**, and from the screen otherwise. A failed turn never refreshed the
list, and a running one may already have started more. So "Buzzing · 2 shells"
is still shown, and is now counted from Claude's own list, which also names
subagents and workflows the screen never counted.

Claude's `monitor` entries are not counted (amended 2026-10-24, the day the
engine update put this in front of real workers). Measured on 2.1.310: a
Monitor tool watch is listed as a `shell`, and the `monitor` entries carry an
MCP server and tool. Swarm Next listed one and Platform three for minutes with
no process behind them and no count on Claude's own footer, so counting them
held resting workers at "Buzzing · 3 monitors". A Monitor watch now reads as a
shell once its turn ends. The engine installed before the amendment still
sends a monitor count; the API ignores it, so the fix ships with a reload.

A `StopFailure` is reported with its cause: rate limit, API overloaded, login
refused, account or billing, API error. Routing still reads the turn as over.
The roster shows the worker as **Blocked** with "Turn stopped: rate limit" on
the line below, because a resting prompt that will do nothing until a limit
resets is not free for work. A `PermissionDenied` shows as "Auto mode blocked
Bash (Irreversible Local Destruction)" until Claude runs another tool after it,
or the next turn starts.

That first clause was added on 2026-10-25. The rule was "until the next turn
starts", and Platform carried on past a refusal for 45 minutes of one turn
while the roster kept showing it. Measured on 2.1.310 with a session-only
`hard_deny` rule: `PermissionDenied` names the refused call's `tool_use_id`,
and the `PostToolBatch` for that call's own batch arrives right after it,
naming the call too. A later batch that does not name it means Claude moved
on. A refusal Claude stopped on is followed by `Stop` instead, so it stays
until the next prompt: the case the notice exists for. The interview hook
that carries `PostToolBatch` reads at most 64 KiB, so a batch with a larger
output is not seen and the next small one clears the refusal. The host keeps
the id; nothing downstream receives it. The rule runs in the worker engine, so
it takes effect with the next worker engine update.

Codex workers, and Claude workers on an older engine, have no turn report and
read exactly as before.

### 4. Swarm reconnects tools a worker lost

The host reports, per provider session, whether a `swarm-terminal-host
mcp-proxy` process runs directly under it. When a live Claude worker's relay
is gone and the worker is resting, Swarm types `/mcp reconnect all` through
the same guarded delivery as every coordination message: never over unsent
text, a question, or a terminal someone holds.

Once per loss: a worker asked is not asked again until its relay has been seen
running. At most three times per session: a relay that keeps failing (a
refused credential, an API answering 503) would otherwise be asked forever.
While the relay is gone the roster says "Swarm tools disconnected".

### 5. The relay tells Claude when Swarm's tools change

Every response from `/mcp` names the tool revision it serves, in
`x-swarm-tool-surface`. The relay declares `listChanged` in its `initialize`
answer, remembers the revision it last listed, and sends
`notifications/tools/list_changed` the first time a response carries another.
Claude then lists the tools again. ADR 0053 found the server had no channel to
send this on; the relay is that channel, because workers never talk to `/mcp`
directly. The notice follows the worker's next tool call after a reload, so a
worker that just shipped a tool and asks whether the reload landed can then
call it.

## Consequences

- None of the hook work, the relay check or the relay notice acts until the
  worker engine update. Before it, sessions have no turn report and no relay
  status, and every worker reads as it does today. The engine update restarts
  every worker with the new hooks; the API side ships with a reload.
- The screen reader is not retired. It still owns menus, unsent input, what an
  interrupt looks like, and every provider without hooks.
- An operator setting of `disableAllHooks` turns all of this off for that
  worker, silently, and it reads from the screen alone.
- The end-of-turn line and the quiet minute are screen readings, and a Claude
  release can change the first. The quiet minute bounds the damage to one
  minute of a worker shown busy after an interrupt.
- A rate-limited worker still receives coordination messages. Claude keeps
  them in the conversation and carries on after the reset; holding them is a
  separate decision.
- `/mcp reconnect all` retries every failed server for that worker, not only
  Swarm's. It is what an operator would type.
