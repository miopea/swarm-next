# 0126 — An outside tool files work by intake

- Status: Accepted
- Date: 2026-10-27
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-27.
  They pasted requests from Claude Duet, which manages their mail and Jira
  through Swarm's MCP connector. Offered three levels of visibility, they chose
  "its tasks + titles". Offered building now or writing it up first, they chose
  building now.
- Extends: ADR 0013 (scoped agent application bridge) and `docs/39`
  (connecting an outside tool).

## Context

`docs/39` gave a connected tool a worker's surface: its own profile, role
Worker, refused approve and assign. That surface was built for a worker with a
repository and an assignment, and a connected tool has neither.

- `swarm_create_task` required `workspace`. Duet reads email and Jira, so it
  often cannot know which repository the work belongs to.
- `swarm_list_tasks` returned only the caller's current assignment, and a tool
  never has one. Duet could not read back what it had just filed.
- It could not see which workers exist, so it could not suggest one.
- It could not check whether the work was already on the board.
- Where the work came from (an email, a Jira key, who asked) had nowhere to go
  except the description.

## Decision

Every connected tool gets an intake scope. There is no setting: a profile with
a connection client id is a tool, and the scope follows from that.

- **Filing.** `swarm_create_task` takes `workspace` as optional, plus
  `recommended_worker` (a worker's name) and `source` (`link`, `jira_key`,
  `requested_by`). The task still lands as an unassigned draft, and drafts
  already wake Queen. With no repository the task holds the connection
  placeholder `/outside-tool`. Assigning a task already sets its repository
  from the assigned worker's profile, so routing gives it a real one. Unknown
  fields are refused rather than dropped, so a typo is reported to the tool.
- **What it said is kept beside the task.** One table, `task_intake`
  (schema 211), holds whether a repository was named, the recommended worker
  by id, and the source. Every task payload carries it as `intake` with a note
  for the reader.
  - A recommendation must name a worker work can be routed to: not Queen, not
    another tool, not a temporary worker. It is a suggestion; Queen decides.
  - A Jira key is upper-cased and must look like one (`ABC-123`).
- **Reading.** A tool reads in full only the tasks it filed, including closed
  ones, through `swarm_list_tasks` and `swarm_read_task_history`. In full means
  what a board reader sees: the activity log, the recorded evidence, and the
  messages between Queen and the assigned worker about it. The filer is
  read from the `created` activity row, so this holds for work filed before
  intake existed. Acting is unchanged: the gate for transitions, notes and
  evidence still asks about the assignment.
- **Searching.** `swarm_search_tasks` matches words against the titles of open
  tasks, and a Jira key against the key work was filed under. It returns at
  most 20, best first: id, title, state, repository, and whether the searcher
  filed it. It never matches descriptions, because a match on a description is
  a read of it.
- **The roster.** `swarm_list_workers`, for a tool, lists the workers work can
  be routed to: name, description and repository. Nothing about sessions or
  load.
- **Its brief.** A tool is told at connection that it files and Queen routes.
  The worker brief told it its authority was its assignment.

Workers and Queen keep the surface they had. The tool-surface revision is not
bumped, because neither of their tool lists changed.

## Consequences

- Duet can file from an email without guessing a repository, read back what it
  filed, and check for a duplicate first.
- Queen reads the recommendation and the source in the task payload. She routes
  a repository-less draft from its description.
- A tool sees the titles, states and repositories of all open work. The
  operator chose that for duplicate checks. Descriptions stay private.
- Until a repository-less draft is assigned, the board shows `/outside-tool` as
  its repository. The web app does not yet show the source or recommendation.
- A connected client sees the new tools after it reconnects, because an MCP
  client fetches its tool list once per connection.
