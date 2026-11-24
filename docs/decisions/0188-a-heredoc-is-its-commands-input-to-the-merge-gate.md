# 0188 — A heredoc is its command's input to the merge gate

- Status: Proposed. It changes which commands the operator's merge gate
  (ADR 0179) refuses, so it takes effect only on the operator's own decision.
- Date: 2026-11-24
- Amends: ADR 0179, "What it reads" and "What this does not cover".

## Context

ADR 0179's gate reads each command that mentions a merge and runs `gh` or
`curl`. Any such command it cannot parse is refused, because it cannot show
that the command merges nothing.

Its reader had no heredocs. It took `<<'BODY'` for an input redirection and
read the body's lines as shell commands. One apostrophe in the body ("it
doesn't") left a quote open, so the whole command could not be parsed. If the
text said "merge" anywhere, the command was refused as an unreadable merge.

Opening a pull request whose body mentions a merge is a normal thing to do.
A worker's `gh pr create` was refused twice. The first time its body came
from `$(cat <<'BODY' ...)`. The second time a heredoc wrote it to a file
before `gh pr create --body-file`. A body file written without a heredoc
passed. Neither command merged anything.

Reading every body as commands did catch real merges, though: in a heredoc
fed to `sudo -u w bash`, `timeout 60 bash` or `ssh`, or written to a file
and then run as `./m.sh` or `source m.sh`. The gate never names those
programs, and a file can be run in endless ways. So the fix keeps reading
bodies as commands, and changes only what an unreadable body means.

## Decision

- **A heredoc's body is what its command reads on standard input.** It runs
  from the line after its command to the line that is just its delimiter.
  `<<-` takes leading tabs off its lines.
- **Its body is still read as commands.** A merge in it that reads as a
  command counts, wherever the heredoc goes, because a file written from it
  may be run later in the command.
- **A body that cannot be read as commands is data only where nothing may
  run it.** That holds when both of these are true:
  - every program in its pipeline prints, stores or sends what it reads and
    never runs it: `cat`, `tee`, `gh` or `git`;
  - inside a substitution, the command the substitution's output goes to is
    one of those too.

  Then "merge" in the body is not a merge. Anywhere else, an unreadable body
  in a command that mentions a merge is refused, as before. Examples are a
  shell, `sudo`, `ssh`, `python3 -`, `eval "$(...)"` and an assignment.
- **An unquoted heredoc's substitutions run.** When no part of the delimiter
  is quoted, the shell runs the body's `$(...)` and backquotes as it reads
  it. In an unreadable body that is otherwise data, they are still read as
  commands.
- **gh and curl read their request from a heredoc when told to.** That is
  `gh api --input -`, a `gh api` field set to `@-`, or curl's `-d @-`.
  - The heredoc can be the command's own, or one piped to it from
    `cat <<EOF |`.
  - A GraphQL merge mutation in that request is a merge.
  - For the merge API, a `sha` in it names the head as hidden, as
    `curl -d` already did.
- **A substitution inside quotes is read from the same text.** It is no
  longer cut out first by counting parentheses. A `)` inside its quotes or
  heredoc (`1) rename`) closed it early before.
- **`$(( ))` and `(( ))` are arithmetic.** `<<` inside them is a shift. A
  substitution inside arithmetic is not read, so it is refused when the
  command mentions a merge.
- **Everything else still fails closed.** A command that mentions a merge is
  refused as unreadable when it has any of these:
  - a quote, a substitution or a heredoc never closed;
  - a heredoc with no delimiter;
  - a substitution that closes over a heredoc whose body never came.

## What this does not cover

- **One case 1.25.0 refused is now let through.** A heredoc that cannot be
  read as commands is written by `cat` or `tee` to a file, and the file is
  then run. In that one case a merge on an earlier line of the body is not
  seen. 1.25.0 refused it only because the body could not be parsed. A body
  that can be read is still read.
- **Already outside what the gate reads (ADR 0179):**
  - a here-string (`bash <<< 'gh pr merge 5'`). Its word is passed over as a
    redirection's file, as it always was;
  - a program that runs code it is given, such as
    `python3 -c "os.system('gh pr merge 5')"`;
  - a script run from a file, as in `bash < m.sh`, or with text piped into
    it, as in `echo '...' | sh`;
  - `eval "$var"`, and an alias set from a heredoc and run later.

## Evidence

Each command below ran through the gate's own entry point,
`swarm-terminal-host provider-merge`. The installed engine was 1.25.0, from
commit `be2c994d`. gh was not on PATH, so a merge the gate finds is refused
as unreadable and no call reached GitHub.

| Command | 1.25.0 engine | This change |
| --- | --- | --- |
| `gh pr create --body "$(cat <<'BODY' ... BODY )"` | refuse | pass |
| `cat > body.md <<'BODY' ... BODY` then `gh pr create --body-file body.md` | refuse | pass |
| `gh pr create --body-file <file>` | pass | pass |
| `git commit -m "$(cat <<'EOF' ... Don't merge yet ... EOF )"` | refuse | pass |
| `gh pr merge 5 --subject "it's` | refuse | refuse |
| `bash <<'EOF'` with an unparsable `gh pr merge 5` | refuse | refuse |
| `cat <<'EOF' \| sh`, `bash`, `nohup bash`, `sudo -u w bash`, `timeout 60 bash`, `ssh host`, `docker exec -i c sh`, `su - w`, `ksh`, `{ bash; }`, `sh -c "$(cat <<'EOF' ...)"` or `x=$(cat <<'EOF' ...)`, each with a heredoc holding `gh pr merge 5` | refuse | refuse |
| `cat > m.sh <<'EOF'` holding `gh pr merge 5`, then `./m.sh`, `source m.sh`, `. ./m.sh` or `bash m.sh` | refuse | refuse |
| `cat <<'EOF' \| gh api graphql --input -` with a merge mutation | pass | refuse |
| `bash ./scripts/check.sh && gh pr view 5 --json mergeable` | pass | pass |
| `gh pr view 5 --json mergeable; echo $((1<<2))` | pass | pass |

Two earlier versions of this change failed their verification:
- The first made every heredoc data unless a bare shell read it. Heredocs
  fed to `sudo`, `timeout`, `ssh`, `docker`, `su` and `ksh` then passed.
- The second treated a heredoc as data whenever its pipeline held only
  `cat`, `tee`, `gh` or `git`. A script written that way and run by path or
  `source` then passed. Its script-file rule also refused
  `bash ./scripts/check.sh && gh pr view 5 --json mergeable`.

This decision is the version that passed.

Nineteen pieces were removed one at a time, and a named test failed each
time.

## Consequences

- A worker can open, edit or comment on a pull request, or commit, with a
  heredoc body that mentions a merge, given to `gh`, `git`, `cat` or `tee`.
- A pull request body containing a line that reads as a merge command, such
  as `` `gh pr merge 5` ``, still counts as that merge. 1.25.0 did the same.
- The change is in `swarm-terminal-host`. It reaches a worker only after the
  worker engine update, which stops running sessions.
