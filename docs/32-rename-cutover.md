# Renaming the identifiers: the cutover

The product is called Swarm. Every user-visible name already says so. What
remains are the identifiers the machine uses, and they cannot be changed while
the Hive is running.

## What is already done

All 135 user-visible occurrences, across the web UI, the API's own output, the
Legacy migration copy, `README.md`, `AGENTS.md` and every document under
`docs/`. There is no runtime risk in any of it: nothing reads a display string
to find a file, a service, or a server.

The crate names never needed changing — they are `swarm-domain`,
`swarm-persistence`, `swarm-api`, `swarm-terminal`, `swarm-terminal-host`,
`swarm-cli` and `swarm-application` already.

## What remains, and why it is not done yet

Five identifiers, and each one breaks something while it changes.

| Identifier | Occurrences | What breaks during the change |
| --- | --- | --- |
| `~/.local/state/swarm-next/` and `swarm-next.sqlite3` | 56 | The live database. Every task, decision, worker and email thread. |
| `swarm-next-api.service` | 39 | The API stops. |
| `swarm-next-terminal-host.service` | 33 | Every worker's terminal. |
| `swarm-next-host-reconcile` and `-development-reload` units | 32 | The reload and reconcile paths. |
| MCP server key `swarm-next` | 5 | **Every running worker's Swarm access, at once.** |

The last one is why this is a cutover rather than a commit. A worker's MCP tool
schema is fixed when its session connects. Renaming the key does not degrade a
running worker's access — it removes it, for every worker simultaneously, until
each one is restarted. That was demonstrated on this worker: three tools added
mid-session stayed invisible until the session reconnected, and restarting the
API was not enough.

The state directory is why this needs a rehearsal rather than a script run once
in anger. It holds the only copy of the Hive.

## The order, and why it is this order

1. **Stop accepting new work.** The operator picks the moment. Nothing here
   should run while they are mid-task, because step 5 ends every session.
2. **Back up the database first**, to a path outside both the old and the new
   directory, and verify the backup opens and reports the expected schema
   version. A backup nobody opened is a hope.
3. **Stop the services** in dependency order: reconcile timer, development
   reload path, API, terminal host.
4. **Move the state directory and the database file**, then re-point the
   configuration. Move rather than copy — two directories, one of them stale, is
   the failure that outlives the rename.
5. **Install the renamed units, rename the MCP key, restart.** Every worker then
   has to be woken so it reconnects on the new key.
6. **Verify from the machine, not the build:** `/health` answers and names the
   expected revision, the roster lists every worker, and one worker can actually
   call a Swarm tool.

## The rollback, which must exist before step 3

Keep the old unit files and the old MCP key definition until step 6 passes. If
anything fails, the way back is to restore the units, restore the key, move the
state directory back, and restart — with the verified backup as the floor.

## Why this is not scripted end to end

It could be, and it would read as one confident command. It should not be. The
operator has to choose the moment, watch step 2 succeed, and be present while
every worker is restarted. A one-command rename of a live Hive's only database
is the kind of thing that works nineteen times.

Ask for the cutover when the fleet can go down. It is perhaps twenty minutes,
most of it waiting for workers to come back.
