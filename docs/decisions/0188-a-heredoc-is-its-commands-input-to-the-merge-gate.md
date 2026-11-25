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
  The gate reads the body where bash does:
  - The delimiter loses its quotes as bash takes them off. In double quotes a
    backslash stays unless it escapes `$`, a backquote, `"`, a backslash or a
    newline. A backslash and newline join two lines and quote nothing.
  - A delimiter that bash reads in a way the gate does not is not read:
    `$'...'`, `$"..."`, a `$(`, `${` or `$[`, or a backquote.
  - In an unquoted body, a backslash and newline join two lines, before the
    line is compared with the delimiter.
  - `<<-` takes the leading tabs off each line once it is joined.
  - Inside `$(...)`, bash 5.2 also ends the body at a line that starts with
    the delimiter and goes on, such as `EOF)`. Such a body is not read.
- **Its body is still read as commands.** A merge in it that reads as a
  command counts, wherever the heredoc goes, because a file written from it
  may be run later in the command.
- **A body that cannot be read as commands is data only where nothing in the
  command can run what it is given.** That holds when nothing anywhere in the
  command reads a value as code:
  - no process substitution;
  - no arithmetic that names a variable, as in `$((x))`, `(( x ))` or
    `$[x]`, and no `${...}`. Bash reads such a value as arithmetic or as a
    name, and a subscript in it, such as `a[$(bash m.sh)]`, runs commands.
    Arithmetic of decimal numbers and operators alone is fine. Any letter
    counts as a name, so a hex number such as `0x1F` or a base such as
    `16#ff` is refused too.

  Every simple command in it must also be one of the following:
  - `cat`, `tee`, `echo`, `printf`, `cd`, `pwd`, `true`, `false`, `:`,
    `mkdir`, `ls` or `rm`;
  - `gh` with `pr`, `issue`, `release`, `gist`, `api`, `label`, `search` or
    `repo`;
  - `git`, after nothing but `-C <folder>` or `--no-pager`, with `add`,
    `commit`, `push`, `status`, `diff`, `log`, `show`, `tag`, `notes`,
    `fetch`, `rev-parse`, `hash-object`, `branch`, `switch` or `checkout`.

  "Every simple command" means the whole command text, including the
  commands inside its substitutions and those an unquoted body runs as it is
  read.
  - Each program must be named as written, bare or under `/bin`, `/usr/bin`
    or `/usr/local/bin`. A wrapper, an assignment before it, a path of its
    own such as `./cat`, or a name that bash computes (below) does not
    count.
  - `printf` does not count with `-v`, or when bash computes its first word.
    `-v` names a variable, and a subscript in that name runs commands.
  - For gh and git, a word bash computes counts only as the value of a text
    flag where gh or git reads it as a flag (below), and never when it may
    split. Anywhere else it could be any of their commands or flags, such as
    `gh pr checkout`, which runs git and its hooks.
  - gh does not count with `--`, after which `gh repo clone` hands words to
    git.
  - gh does not count when it may run the editor or the browser its config
    names, which the same command can write, as
    `cat > ~/.config/gh/config.yml <<'B'` with `editor: sh -c '...'`. That
    is `-e` or `--editor`, `-w` or `--web`, alone or among one-letter flags
    before one that takes a value, and `gh gist edit`, which opens the
    editor with no flag.
  - git does not count with a long flag that names a program for it to run:
    `--receive-pack`, `--exec` or `--upload-pack`. git takes any start of a
    long flag's name that only one flag begins with, so `--rec` counts as
    `--receive-pack`.
  - **A gh or git given standard input counts only when it reads all of it
    as text or a request first.** Standard input here is a heredoc, a pipe,
    or a file, here-string or descriptor after `<`. An editor that git or gh
    opens reads the same input, and an editor such as vim runs `:!` lines it
    reads. A harmless editor confirmed that git hands its input to the
    editor, for `git commit` and `git tag -a`. gh's `--editor` was not run.
    - gh counts with `--body-file -`, `-F -`, `--input -` or a field's
      `@-`.
    - git counts with `-F -`, `-F-`, `--file -` or `--file=-`. With these,
      git read all of it as the message before its hooks and its editor ran,
      which then read nothing, also with `-e`.
    - git does not count with `--patch` or `--interactive`, any start of
      them, or a short flag holding `p`. These read the input first, and
      `e` there opens the editor.
  - The rule passes down. A body that a shell runs, or text that `bash -c`
    runs, is never data when the command around it is not. Otherwise bash
    could run a body that writes a script, and a later command could run it.

  Then "merge" in such a body is not a merge. Anywhere else, an unreadable
  body in a command that mentions a merge is refused, as before.
- **What a substitution prints is words of its command.** The commands
  inside a substitution are read after the command that holds it, so that
  command keeps all its words. `--match-head-commit` or `--repo` after a
  `--body "$(...)"` is read as part of the merge.

  A heredoc in a substitution is data only when the substitution is in
  double quotes and is the value of a text flag, of a command that takes
  text:
  - gh's `--body`, `-b`, `--title`, `-t`, `--notes`, `-n` and `--subject`,
    for `pr create`, `edit`, `comment`, `review` and `merge`, `issue
    create`, `edit` and `comment`, and `release create` and `edit`;
  - git's `-m` and `--message`, for `commit`, `tag` and `notes`.

  The `--flag=` form counts too. Any other command may take its flags'
  values another way: `gh api` has no `-b`, and `git fetch -m` takes no
  value.
  - The flag counts only where gh or git reads it as a flag. That is first
    after the command, or after a word that is not a flag, a flag with its
    value attached, a flag that takes no value (such as `--draft` or `-a`),
    or another text flag's value. Nothing before it may be `--`,
    `--end-of-options`, or a computed word that is not text. After
    `--template`, `-q`, `-o` or `--trailer`, the `-b` or `-m` that follows
    is that flag's value, and the printed word is an endpoint or a flag.
  - The gate decides this as it reads the substitution, from the words
    before it. In `"$(...)"--message=x`, what is printed comes before the
    flag's name and is not its value.

  Anywhere else the heredoc is read as commands in place, so an unreadable
  one is refused. That covers an endpoint, a field value, a command's
  arguments and an assignment.
- **Every word bash computes counts as a printed one does.** A word is
  computed when bash works out part of it as it runs the command. That part
  can be:
  - what a substitution prints;
  - a variable, such as `$_`, the last word of the command before;
  - a `~` at its start;
  - `$'...'` or `$"..."`;
  - a brace list, such as `{a,b}` or `{a..c}`;
  - a glob: `*`, `?`, or `[` and then `]`.

  Bash 5.2 turned `{--,}` into `--`, `-{o..q}` into `-o -p -q`, `$'-p'`
  into `-p`, and `"$_"` after `: --receive-pack=x` into that flag. A brace
  with no list, such as gh's `{owner}`, stays as written. Unquoted, a
  variable, a brace list or a glob may be any number of words, or none.
  `$'...'` ends at its own quote, which a backslash escapes. An extglob
  pattern such as `!(x)` is not read, so a command with one that mentions a
  merge is refused.
- **An unquoted heredoc's substitutions run.** When no part of the delimiter
  is quoted, the shell runs the body's `$(...)` and backquotes as it reads
  it, and evaluates its arithmetic and `${...}`. In an unreadable body that
  is otherwise data, they are still read as commands. In any body, they are
  part of the command that decides whether a body may be data.
- **gh and curl read their request from a heredoc when told to.** That is
  `gh api --input -`, a `gh api` field set to `@-`, or curl's `-d @-`.
  - The heredoc can be the command's own, or one piped to it from
    `cat <<EOF |`.
  - A GraphQL merge mutation in that request is a merge.
  - For the merge API, a `sha` in it names the head as hidden, as
    `curl -d` already did.
- **A GraphQL request the gate cannot see is held as a merge it cannot
  read.** It is refused as 1.25.0 refused it, with the note to give the
  request in the command itself. That is:
  - `gh api graphql` reading its request from a file: a field's `key=@file`
    after `-F` or `--field`, or `--input file`. `/dev/fd/0` counts as a
    file. `-f` sends an `@` as it is.
  - `gh api graphql` sending standard input the gate cannot see: a file, a
    here-string or a descriptor after `<`, a pipe from anything but `cat`
    with no file or `tee`, or nothing at all. A heredoc is seen when it is
    the command's own, or passed down the pipe by `cat` or `tee`.
  - curl sending `-d`, `--data`, `--data-binary` or `--json` from `@file`,
    or from `@-` with such input, to GitHub's GraphQL endpoint. `--data-raw`
    sends an `@` as it is.
  - curl given `-K` or `--config`, whose file can hold the URL and the data.

  A REST merge names its endpoint, which the gate reads, so a REST request
  with its body in a file is not held.
- **A substitution inside quotes is read from the same text.** It is no
  longer cut out first by counting parentheses. A `)` inside its quotes or
  heredoc (`1) rename`) closed it early before.
- **`(( ))` and `$(( ))` are arithmetic only where bash reads them so.** The
  `)` that closes the second `(` must be followed at once by another `)`.
  - Then `<<` inside is a shift.
  - Otherwise bash runs a subshell, and the gate reads one:
    `((gh pr merge 5) )` and `x=$((gh pr merge 5) )` are merges.
  - Arithmetic that holds a quote, an escape, a backquote or a substitution
    is not read, so it is refused when the command mentions a merge.
- **Everything else still fails closed.** A command that mentions a merge is
  refused as unreadable when it has any of these:
  - a quote, a substitution or a heredoc never closed;
  - a heredoc with no delimiter;
  - a substitution that closes over a heredoc whose body never came.

## What this does not cover

- **A file written from an unreadable body is not followed when something
  runs it without the command naming it.** Three such cases pass that 1.25.0
  refused:
  - the file is a git hook, then run by a later `git push` or `git commit`
    in the same command;
  - the file is a git config naming a program, such as `core.fsmonitor`,
    then run by a later `git status`;
  - the file is run by a later, separate command.

  1.25.0 refused these only because it misread the body as shell. It passes
  the same file written by `printf` (`printf 'gh pr merge 5\n' > m.sh; bash
  m.sh`), because the gate has never followed a file. A body that can be
  read is still read, wherever it goes.
- **Merges that 1.25.0 and this change both pass.** These are outside this
  change and are to be fixed separately:
  - a substitution printing a merge's words, as in `gh $(echo pr merge 5)`,
    `gh pr $(echo) merge 5`, or a GraphQL merge in
    `gh api graphql -f query="$(cat <<'EOF' ... EOF)"`;
  - a GraphQL mutation name split by an expansion in an unquoted heredoc
    (`merge${x}PullRequest`);
  - a merge's words spelled through `$'...'` or a brace list, as in
    `gh pr $'merge' 5` or `{gh,pr,merge,5}`;
  - a process substitution's commands, as in `cat <(gh pr merge 5)`, whose
    first word is still taken as the redirection's file.
- **Found while fixing the second review, and passing on this change.** The
  code that reads them is unchanged since 1.25.0, which passes them too. To
  be fixed separately:
  - a GraphQL merge sent to the REST path `/graphql`, as in
    `gh api /graphql -f query='mutation { mergePullRequest(...) }'`;
  - curl's data attached to its flag, as in `curl .../graphql -d'{...}'`;
  - the branch merges API, `gh api -X POST repos/o/r/merges`, which merges
    a branch, such as a pull request's head, into another;
  - an unreadable command that mentions `enqueuePullRequest` but not
    "merge". The hook sends it to the gate, which looks only for "merge";
  - curl's own config, `~/.curlrc`, which curl reads unless told not to,
    written by the same command with the data or the URL, then a bare
    `curl`.
- **gh's other ways to run a program its config names.** Its pager runs only
  on a terminal, and an interactive prompt that opens the editor needs one
  too. A Claude worker's command has none: none of its standard input,
  output or error is a terminal. Neither was run. Nor was whether gh opens
  its editor for `-e` or `gist edit` without a terminal: the gate holds them
  as if it does.
- **Already outside what the gate reads (ADR 0179):**
  - a here-string (`bash <<< 'gh pr merge 5'`). Its word is passed over as a
    redirection's file, as it always was. Sent by gh or curl as a request, it
    is held as one the gate cannot see;
  - a request read from a file in a command that does not mention a merge,
    as `gh api graphql -F query=@q.graphql` after a separate command wrote
    it. The hook sends the gate only commands that mention a merge;
  - a program that runs code it is given, such as
    `python3 -c "os.system('gh pr merge 5')"`;
  - a script run from a file, as in `bash < m.sh`, or with text piped into
    it, as in `echo '...' | sh`;
  - `eval "$var"`, and an alias set from a heredoc and run later.

## Evidence

Each command below ran through the gate's own entry point,
`swarm-terminal-host provider-merge`. The installed engine was 1.25.0, from
commit `be2c994d`. gh was not on PATH, so a merge the gate finds is refused
as unreadable and no call reached GitHub. Bash 5.2.21 was run on the shell
forms the rule rests on, to confirm what it runs: arithmetic, delimiters,
joined lines, a delimiter line inside a substitution, process substitutions,
subshells and groups piped to `bash`, and a subscript run through `$((_))`,
`$[_]`, `${!_}`, `${x@P}`, an unquoted body and `printf -v`. It was also run
on the words it computes: brace lists, `$'...'`, `$_`, globs, and extglob
with the option on and off. git 2.43.0 was
run with a harmless editor and hooks, to confirm that its editor reads its
standard input and that `-F -` leaves none for them. It was also run with a
harmless program, and ran it when `git push -o -m` was followed by
`--receive-pack=<program>`, and when one word was
`--receive-pack=<program> --message=x`. Every flag the rule treats as
taking no value was checked against the help of gh 2.103.3 and git 2.43.0,
for each command that takes text. gh's own reading of `--template -b` was
not run.

| Command | 1.25.0 engine | This change |
| --- | --- | --- |
| `gh pr create --body "$(cat <<'BODY' ... BODY )"` | refuse | pass |
| `cat > body.md <<'BODY' ... BODY` then `gh pr create --body-file body.md` | refuse | pass |
| `gh pr create --body-file <file>` | pass | pass |
| `git commit -m "$(cat <<'EOF' ... Don't merge yet ... EOF )"`, alone or with `git add` and `git push` | refuse | pass |
| `gh pr merge 5 --subject "it's` | refuse | refuse |
| `bash <<'EOF'` with an unparsable `gh pr merge 5` | refuse | refuse |
| `cat <<'EOF' \| sh`, `bash`, `nohup bash`, `sudo -u w bash`, `timeout 60 bash`, `ssh host`, `docker exec -i c sh`, `su - w`, `ksh`, `{ bash; }`, `sh -c "$(cat <<'EOF' ...)"` or `x=$(cat <<'EOF' ...)`, each with a heredoc holding `gh pr merge 5` | refuse | refuse |
| `cat > m.sh <<'EOF'` holding `gh pr merge 5`, then `./m.sh`, `source m.sh`, `. ./m.sh` or `bash m.sh` | refuse | refuse |
| `((gh pr merge 5) )`, `x=$((gh pr merge 5) )`, and the same in double quotes or an unquoted body | refuse | refuse |
| A delimiter of `"E\OF"`, `$'EOF'`, or `E\` and `OF` joined, each hiding `gh pr merge 5` after the line bash ends the body at | refuse | refuse |
| `cat <<'B' > >(bash)`, `\| tee >(bash)`, or after `exec > >(bash)`, with an unreadable body holding `gh pr merge 5` | refuse | refuse |
| The same body given through `( cat ) \| bash`, a `{ }` group, an `if` block or a function piped to `bash`, a fifo, `gh codespace ssh`, `git -c alias.x='!bash' x`, or `cat <<'B' $(true) \| bash` | refuse | refuse |
| `gh $(cat <<'B' ... pr merge 5 ... B)`, and `gh api -X PUT "$(cat <<'B' ... B)"` with the merge endpoint | refuse | refuse |
| A script written by `cat > m.sh` and run by `bash m.sh`, or a gh alias set from a heredoc and run, with an unreadable line in the body | refuse | refuse |
| A heredoc body closed on a line `EOF)` inside a substitution, hiding `gh pr merge 5` | refuse | refuse |
| A script written from an unreadable body, then run by `echo 'a[$(bash m.sh)]'` followed by `$((_))`, `$[_]`, `${!_}` or `$((_))` in an unquoted body, by `printf -v 'a[$(bash m.sh)]'` or a printed `-v`, or by `$(bash m.sh)` in an unquoted body | refuse | refuse |
| An unreadable body holding `:!gh pr merge 5` given to `git commit`, `git tag -a` or `gh pr comment --editor`, as a heredoc, through a pipe or as `< m.txt`, or to `git commit -p -F -` or `--inter -F -` | refuse | refuse |
| A script written from an unreadable body, then named by `git push --receive-pack=`, `--rec=` or a printed flag, or by `gh repo clone o/r -- --upload-pack=` or a printed `--` | refuse | refuse |
| `gh api -X PUT --template -b "$(cat <<'B' ... B)"` or `-q -b` with the merge endpoint; a script written from an unreadable body, then named by a printed `--receive-pack=` after `git push -o -m` or before `--message=x`; `git commit -F - --trailer -m "$(echo --patch)"` given `:!gh pr merge 5` | refuse | refuse |
| A script written from an unreadable body, then named by `git push` through `{"--receive-pack=bash m.sh",}`, `$'--receive-pack=bash m.sh'`, `--receive-pa$'ck=bash m.sh'`, `"$_"` after `: --receive-pack='bash m.sh'`, or `--rece*` or `!(cat)` matching a file of that name, or run by `printf {-v,}` or `gh repo clone o/r {--,}`; `git commit -F -` with `{--trailer,} -m "$(echo --patch)"`, `$'-p'` or `-{o..q}`, given `:!gh pr merge 5` | refuse | refuse |
| `git commit -F - <<'EOF'`, `--file=-`, `cat <<'EOF' \| git commit -F -`, `git tag -a v1 -F-`, `gh pr comment --body-file -` or `gh issue comment -F -`, each with "merge" and an apostrophe | refuse | pass |
| `git commit -a -m "$(cat <<'EOF' ... EOF )"`, `gh pr create --draft --title "$(...)" --body "$(...)"` or `gh pr comment 5 --body "$(...)"`, each with "merge" and an apostrophe | refuse | pass |
| `gh pr create --title "$TITLE" --body "$(cat <<'EOF' ... EOF )"`, or `gh api repos/{owner}/{repo}/issues/5/comments -F body=@-` with a heredoc, each with "merge" and an apostrophe | refuse | pass |
| A git config written from an unreadable body, naming a script as `core.fsmonitor`, then `git status` | refuse | pass |
| `x="$(true)" gh pr merge 5` | pass | refuse |
| `echo $'\'' ; gh pr merge 5 #'`, where `$'\''` is one quote | pass | refuse |
| `cat <<'EOF' \| gh api graphql --input -` with a merge mutation | pass | refuse |
| A merge mutation written from an unreadable body to a file by `cat` or `tee`, then sent by `gh api graphql -F query=@q.graphql` or `cat q.graphql \| gh api graphql -F query=@-` | refuse | refuse |
| A merge mutation in a file written from a body readable as shell or by `printf`, then sent by `-F query=@q.graphql`, `--input q.json` or `--input - < q.json`; or curl's `-d @q.json` to the GraphQL endpoint | pass | refuse |
| A gh config written from an unreadable body naming an editor or browser, then `gh pr comment 5 -e`, `gh pr view 5 --web` or `gh gist edit 1234` | refuse | refuse |
| `bash ./scripts/check.sh && gh pr view 5 --json mergeable` | pass | pass |
| `gh pr view 5 --json mergeable; echo $((1<<2))` | pass | pass |

Three earlier versions of this change failed their verification:
- The first made every heredoc data unless a bare shell read it. Heredocs
  fed to `sudo`, `timeout`, `ssh`, `docker`, `su` and `ksh` then passed.
- The second treated a heredoc as data whenever its pipeline held only
  `cat`, `tee`, `gh` or `git`. A script written that way and run by path or
  `source` then passed. Its script-file rule also refused
  `bash ./scripts/check.sh && gh pr view 5 --json mergeable`.
- The third kept that pipeline rule. An independent review found real
  merges it passed that 1.25.0 refused, 11 forms in all. Reading the code
  for the review found 12 more, and reading it again after fixing those
  found 19 more. Verifying the result found 5 more, and verifying that
  found 10 more. Every row from `((gh pr merge 5) )` down to the
  `{"--receive-pack=bash m.sh",}` row above is one of them. Six causes
  covered them:
  - arithmetic and delimiters were read differently from bash;
  - a substitution split its command into pieces, so each piece was judged
    alone;
  - the rule looked only at a body's own pipeline, never the rest of the
    command;
  - a command on the list could still run code: bash reading a value as
    arithmetic or a name, an editor reading the same standard input, or a
    flag naming a program;
  - a text flag counted wherever it stood, even where gh or git reads it as
    the value of the flag before it, and for commands where it takes no text;
  - a word bash computes from a variable, `$'...'`, a brace list or a glob
    was read as written, so the gate never saw the flag bash made of it.

The fourth version passed that verification and its first review. Its
second review found two classes that 1.25.0 refused and it passed: a
GraphQL request read from a file the same command wrote, and a gh config
the same command wrote naming an editor or browser. 1.25.0 refused both
only because it misread the body as shell. This decision is the fourth
version with both closed, and it has not been reviewed again. Both columns
of those three rows were run on the built gates, the installed 1.25.0 and
this change, with gh out of reach.

A hundred and nineteen pieces were removed one at a time, and a named test
failed each time. Twenty-three more were removed for the two classes, with
the same result.

## Consequences

- A worker can open, edit or comment on a pull request, or commit, with a
  heredoc body that mentions a merge. The body is given to `cat`, `tee`,
  `gh` or `git` directly, or through the quoted value of a text flag, in a
  command whose other commands are of the same kind.
- A pull request body containing a line that reads as a merge command, such
  as `` `gh pr merge 5` ``, still counts as that merge. 1.25.0 did the same.
- Some commands that merge nothing are still refused, as 1.25.0 refused
  them, when they mention a merge and hold an unreadable heredoc. Examples:
  - a heredoc in a `gh api` field set from `"$(cat <<'EOF' ...)"`;
  - one inside `bash -c`, or in a command with an assignment before it;
  - one in a command that also holds `${...}` or arithmetic naming a
    variable;
  - one given to gh or git on standard input without one of the flags that
    read all of it, such as `gh pr view` or `git commit` with no `-F -`;
  - one in a text flag's value right after a flag the gate does not know to
    take no value, such as `git commit -v -m "$(...)"`, or in a short flag
    group such as `-am`, or for a command it does not list, such as
    `gh pr new`;
  - one in a command whose gh or git has a word bash computes anywhere but
    a text flag's quoted value, such as `git add docs/*.md`,
    `git -C "$HOME/x"` or `gh pr create --base "$BASE"`.
- A GraphQL request read from a file, or from standard input the gate cannot
  see, is refused in a command that mentions a merge, even when it merges
  nothing, such as
  `gh api graphql -F query=@q.graphql --jq .data.repository.pullRequest.mergeable`.
  The request has to be in the command, as a field or a heredoc.
- An unreadable heredoc in a command with `gh pr view --web`,
  `gh pr comment -e` or `gh gist edit` is refused when the command mentions
  a merge.
- The change is in `swarm-terminal-host`. It reaches a worker only after the
  worker engine update, which stops running sessions.
