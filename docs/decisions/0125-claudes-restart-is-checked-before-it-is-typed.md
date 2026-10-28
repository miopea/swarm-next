# 0125 — Swarm checks that Claude's `/restart` is on before typing it

- Status: Accepted
- Date: 2026-10-26
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-26.
  They showed a worker with two "Unknown command: /restart" lines and said:
  "This was attempted even in your own chat and it didn't work". Offered
  reading Claude's flag first or learning from Claude's reply, they chose
  reading the flag, with the reply as a backstop.
- Amends: ADR 0122 (Claude workers move onto a new Claude release in place).

## Context

ADR 0122 moves a Claude worker onto a newly installed release by typing
`/restart`. That command is undocumented. Claude enables it only while a
feature flag is on for the account.

Measured on 2026-10-26:

- Claude 2.1.311 installed at 21:50Z.
- Swarm asked six workers at 21:50:51–59Z, and the same five again at
  22:04:37–42Z.
- Every ask was answered "Unknown command: /restart". The answer is recorded in
  each worker's transcript.
- Every installed binary, 2.1.308 through 2.1.311, still defines the command.
  It is enabled while the flag `tengu_fancy_wand` is on.
- The copy of that flag Claude caches in `~/.claude.json` read `false`.

ADR 0122's one-ask rule kept this to one ask per worker per release. But its
Restart button asks a Claude worker again in place instead of stopping it, so
the operator had no way left to move these workers. Each press typed another
command Claude did not have.

## Decision

1. The terminal host reads the flag where Claude runs (ADR 0109). The value is
   `cachedGrowthBookFeatures.tengu_fancy_wand` in Claude's `.claude.json`,
   taken from `CLAUDE_CONFIG_DIR` when that is set and from the home directory
   otherwise. The host reports it as `claude_restart`, beside the Claude
   release. The field is defaulted, so there is no protocol change. An older
   host reports nothing, and nothing is not "off".
2. While the flag reads `false`, Swarm types no `/restart`. The Claude workers
   behind the release are counted with the workers that need stopping, so the
   notice keeps its warning and Restart stops and starts them, as it did before
   ADR 0122. The Updates view and the Restart button read one shared answer.
3. The backstop: a missing or renamed flag is no evidence, so Swarm asks as
   before. Suppose a worker behind the release shows Claude's own line
   `● Unknown command: /restart`. That ends in-place updates for that release,
   held in memory. Swarm reads the workers already asked, and also the resting
   workers it is about to ask. An API restart forgets every ask, and a worker
   still showing the answer must not be typed into again. Swarm matches the
   line Claude draws, not the words anywhere on screen: a worker discussing
   this failure has the same words in its own prose. A newer release is asked
   about afresh.

## Consequences

- The flag read acts only after the worker engine update, because the host
  reports it. Until then the backstop decides. A worker that has scrolled the
  answer away is asked once more, and the first answer stops the rest.
- The flag's name is Claude's internal name and can change in any release. The
  backstop is what keeps a rename from turning back into repeated asks.
- When the command is switched back on, the next read finds it and workers move
  in place again, whatever the backstop marked (see below).

## The flag outranks the screen (2026-10-27)

The operator asked why the Real Truth worker had `/restart`. Claude had switched
the flag on for the account: `~/.claude.json` read `true`. But Swarm's mark on
2.1.312, which nothing but an API restart clears, still counted the workers
behind it as needing stopping.

A second after each API start, at 22:25:13Z and 23:41:21Z, Swarm marked
2.1.312 from an "Unknown command" a worker still showed. So an API restart did
not clear the mark, as the consequence above promised: each restart re-read the
old answer. The flag did not read `false` then, or Swarm would not have looked.
Whether it was unreadable or already `true` is not recorded. Either way, the
mark held after the flag read `true`, because the code let it overrule the flag.

Decision 3 already calls the backstop the answer to a missing or renamed flag.
It now acts only then. A flag Swarm can read decides alone: `false` stops the
asking, and `true` asks once per worker per release, whatever the screen shows.

If Claude ever answers "Unknown command" while its cached flag reads `true`,
each worker is asked once for that release. Restart then asks it once more per
press. That is the ADR 0122 one-ask bound, not the backstop.
