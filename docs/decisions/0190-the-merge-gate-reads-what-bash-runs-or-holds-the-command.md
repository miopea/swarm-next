# 0190 — The merge gate reads what bash runs, or holds the command

- Status: Proposed. It changes which commands the operator's merge gate
  (ADR 0179) refuses, so it takes effect only on the operator's own decision.
- Date: 2026-11-25
- Amends: ADR 0179, "What it reads" and "What this does not cover"; ADR
  0188, "What this does not cover".

## Context

ADR 0179's gate reads each command that mentions a merge and runs `gh` or
`curl`, and refuses a merge it cannot show to be outside the operator's
rule. ADR 0188 listed merges that both it and 1.25.0 pass, to be fixed
separately. Its reviews found more. Each passes on 1.25.0 and on ADR 0188,
or, for the environment and the folder, is checked against a pull request
other than the one bash merges:

- a merge after a reserved word, as `{ gh pr merge 5; }`, `if`, `then`,
  `do`, `else` or `!`, whose first word the gate took for the program;
- a merge behind a wrapper it did not know, as `timeout 60`, `nice`,
  `stdbuf`, `env -S` or `exec -a`;
- a merge another program runs, as `sudo -u w`, `ssh host`, `watch` or
  `find -exec`, which the gate passed over;
- a process substitution, `cat <(gh pr merge 5)`, whose commands were taken
  for a redirection's file;
- a `case` inside `$(...)`, whose pattern's `)` ended the substitution;
- `&>`, `>|` and `>&file`, which split or misread the command;
- a merge spelled with quotes or escapes, as `gh pr mer''ge 5` or
  `gh pr $'\x6derge' 5`, which the grant mod never sent to the gate, and
  `$'merge'`, which the gate read as some other word;
- a word bash computes, as `gh $(echo pr merge 5)` or `gh pr "${V}" 5`;
- a value bash evaluates, where a subscript runs a command, as
  `x='a[$(gh pr merge 5)]'; echo $((x))`;
- a function, or a changed `PATH`, alias or `hash`, so that `cat` in
  `cat <<'C' | gh api graphql --input -` prints something other than its
  heredoc;
- gh told which account, host or repository to use by its environment, as
  `GH_TOKEN`, `GH_HOST` or `GH_REPO`, which the gate's own gh calls do not
  carry;
- a program named by a variable or config, as `GIT_EDITOR`, `git -c alias`,
  `gh alias set` or a shell `alias`;
- curl's flags in a group, attached values, `--data-ascii`,
  `--data-urlencode @file`, `-T`, `-F name=@file`, `-K`, a URL curl expands,
  and its own config, `~/.curlrc`;
- a GraphQL mutation in an unquoted heredoc whose name bash completes, as
  `merge${x}PullRequest`;
- a folder changed by `pushd` or `popd`, or by `cd` to a computed folder.

## Decision

- **A merge is looked for as bash may spell it.** The grant mod and the gate
  each look for "merge" or `enqueuePullRequest`, and for `gh` or `curl`, in
  three spellings of the command:
  - as written;
  - without its quotes and backslashes, a `$` before a quote, an empty `$()`
    or pair of backquotes, and a backslash and newline;
  - the same, after reading each `\xHH`, `\NNN`, `\uHHHH` and `\UHHHHHHHH`
    as the letter it names, as `$'...'`, printf and JSON do.
- **A command whose runs cannot be told from its text is not read.** Where
  the command mentions a merge, it is refused as unreadable, and never
  grantable, when it holds:
  - a value bash evaluates as arithmetic or as a name: `$((x))`, `((x))`,
    `$[x]`, `${a[x]}`, `${!x}`, `${s:x}`, `let`, `[[` with `-eq` and the
    like or `-v`, `declare -i`, `a[x]=`, or a bracketed name given to
    `read`, `unset`, `mapfile`, `readarray`, `getopts`, `wait`, `test`, `[`
    or `printf -v`. `${x}`, `${#x}`, `${x:-y}`, `${x%.md}` and `${x^^}` are
    read as they are, and so is arithmetic that names no variable;
  - a function defined, as `f() {` or `function f {`. `a=()` is an array;
  - a program word bash computes;
  - for gh, a computed word up to its command, and for `gh pr merge` or
    `gh api`, one that may split or is not a whole word;
  - for curl, a computed word that may split or is not the value of `-H`,
    `-u`, `-A`, `-o` or `-w`;
  - for `sudo`, `ssh` and the other programs below, a computed word.

  In a heredoc body that is only printed (ADR 0188), none of these runs, so
  the body does not count.
- **Reserved words and wrappers are read through.** `{`, `!`, `if`, `then`,
  `elif`, `else`, `while`, `until`, `do`, `time` and `time -p`,
  `coproc NAME {`, `command`, `builtin`, `exec` and `exec -a`, `nohup`,
  `nice`, `stdbuf`, `timeout` and `env` each come before the program. A
  folder that `env -C` or `env --chdir` names is the merge's folder, and
  `cd` under a wrapper moves nothing after it. `env -S` runs its string.
- **A merge another program runs is held.** gh or curl run by `sudo`,
  `doas`, `runuser`, `su`, `watch`, `flock`, `ssh`, `setsid`, `chroot`,
  `nsenter`, `unshare` or `parallel`, or among the words of any other
  program the gate does not know, as `find . -exec`, may run as another
  account or on another machine. It is read, and held as a merge the gate
  cannot check, so it must name its head and needs the operator's grant.
- **What gh reads from its environment is held.** A merge in a command that
  sets, exports, declares or unsets a variable gh or git reads to choose an
  account, host or repository, or runs gh under `env -i` or another `env`
  option the gate does not know, is held the same way. Those variables are
  `GH_*` and `GIT_*` other than ones that only change output or prompts,
  `GITHUB_*`, `HOME`, `PATH`, `XDG_CONFIG_HOME` and `CURL_HOME`.
- **A command set up to run in place of another is read.** What these name
  is read as commands of the same command:
  - a variable that names a program, ending in `EDITOR`, `PAGER`, `BROWSER`,
    `ASKPASS` or `SSH_COMMAND`, or `GIT_SSH`, `GIT_EXTERNAL_DIFF`,
    `BASH_ENV`, `ENV`, `PROMPT_COMMAND` or `LESSOPEN`, before a command or
    exported;
  - git's `-c` values and `git config` values;
  - `gh alias set`, with or without `--shell`, and a shell `alias`;
  - `trap`.
- **A name that may not be its program passes nothing on that the gate
  sees.** After a function, a `PATH` change, `alias`, `hash`, `enable`,
  `.`, `source` or `shopt` in the command, `cat` or `tee` piping a heredoc
  into gh may print something else. A request gh or curl reads from
  standard input there is held as one the gate cannot see.
- **A process substitution's commands are read.** `<(...)` and `>(...)` run
  as a word, after digits as `2>(...)`, or as a redirection's file.
- **A `case` pattern's `)` inside a substitution does not end it.**
- **Every redirection bash reads is one.** `&>`, `>|` and `>&file` are read
  as redirections of the command.
- **curl's flags are read as curl reads them.** A group of one-letter flags
  ends at the first that takes a value, which takes the rest of the word or
  the next. Values may be attached. These send a body the gate cannot see,
  so a request with one is held when it may be GraphQL:
  - `-d @file` and its other names, `--data-ascii` among them;
  - `--data-urlencode` with a file;
  - `-T` and `--upload-file`;
  - `-F name=@file` or `name=<file`;
  - `--expand-*`.

  `-K` and `--config` may hold the whole request, and curl reads
  `~/.curlrc` unless `-q` comes first. A URL holding `{` or `[` is a
  pattern curl expands, unless `-g` or `--globoff` is given. It may be
  GraphQL, and a `PUT` to one may be any merge. A mutation named with a
  JSON `\u` escape is still read. A command that writes or names `.curlrc`
  or `CURL_HOME` may make any curl in it send anything, so its requests are
  held.
- **A request read from an unquoted heredoc that bash changes is held.**
  Bash expands `$` and backquotes in an unquoted body, joins a line ending
  in a backslash to the next, and makes `\\` one backslash, which JSON then
  reads with what follows, as in `mer\\u0067e`. So a body holding `$`, a
  backquote or a backslash is not the text gh or curl reads, and a request
  read from it on standard input is held as one the gate cannot see. A
  quoted body is read as written.
- **A folder the gate does not follow is not told.** `pushd` with one
  plain folder is followed. Any other `pushd`, `popd`, and `cd` to a folder
  bash computes, leave the merge's folder untold, so its repository must be
  named.

## What this does not cover

- **A merge spelled only through an expansion whose text never spells it.**
  A brace list or sequence, as `{mer,}ge` or `{l..n}erge`, a glob matching a
  file of that name, a curl URL pattern, as `.../pulls/5/mer{ge,}`, or a
  variable set by an earlier command, in a command that otherwise names no
  merge. The grant mod never sends it to the gate, so the gate cannot hold
  it. The gate does hold `curl -X PUT .../pulls/5/mer{ge,}` when it is sent.
  Whether the mod should send every gh or curl command is an open product
  question (`docs/09-open-questions.md`).
- **A program given a command as one quoted string**, as
  `ssh host 'gh pr merge 5'` or `su -c 'gh pr merge 5' w`. The gate does
  not read the string as commands. `sudo bash -c '...'` is read.
- **A config written before the command, or in a way the gate does not
  read**: gh's config, a git config file given with `--file`, `~/.curlrc`
  written by an earlier command, and `gh alias set NAME -` reading its
  expansion from standard input.
- **A `cd` inside a pipeline, a subshell or a condition.** The gate carries
  it to the commands after it, as 1.25.0 does, where bash does not. A gh
  that only logs its folder showed both asking about the other folder after
  `(cd other)`, `cd other | true` and `false && cd other`. To be fixed
  separately.
- **The branch merges API**, `gh api -X POST repos/o/r/merges` and the same
  sent by curl, which merges one branch into another as a push does. It is
  still to be fixed separately, as ADR 0188 listed it.
- **What the gate now holds instead of reading.** `gh api graphql --input
  /dev/fd/0` is held as a request read from a file it cannot see, not read
  as standard input. A function, and `gh $(echo ...)`, are not followed:
  the command is refused.
- ADR 0188's other limits stand. The ones this decision fixes are listed in
  its Context.

## Evidence

Each of 159 commands, grouped below, ran through the gate's own entry
point, `swarm-terminal-host provider-merge`, on three builds:
- the installed 1.25.0 engine, from commit `be2c994d`;
- `main` before this change, at `8c26cffd`, which holds ADR 0188;
- this change.

gh was not on PATH, so a merge the gate finds is refused as one it could not
check, and no call reached GitHub. That is also why a merge the gate reads
but holds, as with `GH_REPO` or `pushd`, refuses on all three: the unit
tests show which reason each gives. Bash 5.2.21 was run on the forms the
rule rests on, with harmless commands in place of a merge:
- reserved words, `$'...'`, process substitutions, a `case` in `$(...)`;
- `&>`, `>|` and `>&file`, functions, `a=()`;
- an unquoted body whose `$((x))` runs a subscript;
- a body given through `> >(bash)`;
- printf's words: only `-v`'s name runs a subscript; `%d`, `%i`, `%x` and
  `*` take theirs as numbers;
- unquoted bodies: `${x}`, a backquote and a backslash and newline complete
  `merge`, and `mer\\u0067e` comes out as `merge`. A quoted body comes
  out as written.

curl 8.5.0 was run on its flag forms. It refuses `--data=@file` as an
unknown option and exits 2, so that spelling sends nothing.

The grant mod's check was cut from `register.ts` and run under node
24.19.0 on the spelled forms below and on four commands it must not send.
Main's sends none of the thirteen quoted, escaped or `$'...'` forms to the
gate. This change's sends all of them, and still none of the four.

| Command | 1.25.0 | main | This change |
| --- | --- | --- | --- |
| A merge after `{`, `if ... then`, `elif`, `else`, `while`, `until`, `for ... do` or `!` | pass | pass | refuse |
| A merge after `timeout 60`, `timeout -s KILL 60`, `nice -n 5`, `stdbuf -oL`, `exec -a x`, `env -C /tmp`, `$'env'` or `/usr/bin/timeout 60`, or in `env -S '...'` | pass | pass | refuse |
| A merge after `sudo -u w`, `watch`, `flock /tmp/l` or `ssh host`, or in `sudo -u w bash -c '...'` | pass | pass | refuse |
| `cat <(...)`, `cat < <(...)`, `tee >(...)` or `diff <(gh pr view 5) <(...)` holding a merge | pass | pass | refuse |
| `echo "$(case x in x) gh pr merge 5;; esac)"` | pass | pass | refuse |
| The same without the double quotes | refuse | pass | refuse |
| `f() { gh pr "$@"; }; f merge 5`, or with `function f {` | pass | pass | refuse |
| `gh $(echo pr merge 5)`, `gh pr $(echo merge) 5`, `gh {pr,merge} 5`, `gh pr {merge,} 5`, `gh pr $'merge' 5`, `gh pr $'\x6derge' 5`, `gh pr $'mer\147e' 5`, or `V=merge` then `gh pr ${V} 5` or `"${V}"` | pass | pass | refuse |
| `gh api -X PUT repos/o/r/pulls/$N/merge`, `-X $M`, a `$M` holding `-X PUT`, or curl to `.../pulls/$N/merge` | pass | pass | refuse |
| `c='gh pr merge 5'` then `$c`, `eval "$c"` or `bash -c "$c"`; `$(which gh) pr merge 5`, with backquotes too; `/usr/bin/env gh pr merge 5` | pass | pass | refuse |
| `gh pr mer''ge 5`, `mer\ge`, `g''h`, a PUT to `.../mer''ge`, or `mer''gePullRequest`. The 1.25.0 and main grant mods never send these to the gate | refuse | refuse | refuse |
| `gh pr mer$()ge 5` | pass | refuse | refuse |
| The same quoted merges, or one joined by a backslash and newline, or `enqueuePullRequest`, in a command that cannot be parsed | refuse | pass | refuse |
| `cat`, `tee` or `function cat` defined to print a merge mutation, or `export PATH=...`, `hash -p`, `alias cat=` or `. /tmp/x`, then `cat <<'C' \| gh api graphql --input -` | pass | pass | refuse |
| `PATH=/tmp/x:$PATH cat <<'C' \| gh api graphql --input -` | pass | refuse | refuse |
| curl with `-Kq.cfg`, `--data-ascii @q.json`, `--data-urlencode @q.json` or `query@q.graphql`, `-d@q.json`, `-sd @q.json`, `-T q.json`, or `-d @q.json` to `'.../{graphql}'` or `'.../graph[p-q]l'` | pass | pass | refuse |
| curl with `-sX PUT .../pulls/5/merge`, `-d'<mutation>'`, a mutation spelled with `g`, a PUT to `'.../pulls/{5}/merge'`, or `printf 'url = x' > ~/.curlrc` then `curl -d @q.json` | pass | pass | refuse |
| `gh api graphql --input - <<'B'` with a mutation spelled with `g` | pass | pass | refuse |
| `cat q.json \| curl -d @- ...` or `\| gh api graphql --input -` beside `3<<'C'`; `gh api graphql -iF query=@q.graphql` | pass | refuse | refuse |
| `gh pr &>/dev/null merge 5`, and with `&>>`, `>\|`, `1>\|` or `>&/dev/null` | pass | pass | refuse |
| `gh pr {fd}>/dev/null merge 5` | pass | refuse | refuse |
| `x='a[$(gh pr merge 5)]'` then `$((x))`, `(( x ))`, `${a[x]}`, `${s:x}`, `[[ $x -eq 1 ]]`, `let x` or `declare -i y=x` | pass | pass | refuse |
| `Q='mutation { mergePullRequest(...) }'` then `gh api graphql -f query="$Q"` | pass | pass | refuse |
| `git -c alias.m='!gh pr merge 5' m`, `GIT_EDITOR='gh pr merge 5;:' git commit`, `gh alias set m 'pr merge 5' && gh m`, or with `--shell`; `alias m='gh pr merge 5'; m` | pass | pass | refuse |
| ADR 0188's open items: `gh pr $(echo) merge 5`, `{gh,pr,merge,5}`, `gh api graphql -f query="$(cat <<'EOF' ... EOF)"` with a merge mutation, and a merge mutation given to `{ gh api graphql --input -; }`, piped in or as the group's heredoc | pass | pass | refuse |
| A merge mutation in an unquoted heredoc spelled `merge${x}PullRequest` or `mer\\u0067ePullRequest`, given to `gh api graphql --input -` | pass | pass | refuse |
| `echo $'\'' ; gh pr merge 5 # '` | pass | refuse | refuse |
| `GH_REPO`, `GH_TOKEN`, `GH_HOST`, `export GH_REPO` or `env GH_REPO` before a merge; `env --unset=GH_TOKEN`; `pushd /tmp && gh pr merge 5`; `sudo gh pr merge 5`; `env --chdir=/tmp gh pr merge 5` | refuse | refuse | refuse |
| Limits: `ssh host 'gh pr merge 5'`, `su -c 'gh pr merge 5' w`, `gh alias set m - <<'B'` then `gh m` | pass | pass | pass |
| Limit: `curl -X PUT .../pulls/5/mer{ge,}`, which no grant mod sends | pass | pass | refuse |
| Limit: the branch merges API, `gh api -X POST repos/o/r/merges` | pass | pass | pass |
| Cost: `gh api graphql --input - --jq ...mergeable` given an unquoted heredoc with `$N` in its query | pass | pass | refuse |
| Cost: `gh api graphql --input /dev/fd/0 <<'B'` with a plain query | pass | refuse | refuse |
| Cost: `x=1; echo $((x)) && gh pr view 5 --json mergeable`, or a function defined before it | pass | pass | refuse |
| Twenty-four ordinary commands, among them the same request with the delimiter quoted and `$n` as a GraphQL variable, `gh pr view 5 --json mergeable`, `timeout 60 cargo test merge_gate`, `diff <(git show HEAD:a) a`, `echo "$(case x in x) echo hi;; esac)"`, `export PATH=$HOME/.cargo/bin:$PATH && cargo build`, `args=()`, `printf '%s' 'a[1]'`, `GH_HOST=x gh pr view 5`, `nice -n 5 cargo build` and `curl --variable` | pass | pass | pass |

Eighteen tests are new, and each passes on this change. The seventeen for
the gate fail on `main`'s gate. The grant mod's fails on `main`'s module,
which holds none of the text it looks for. Seventy-two
pieces of the change were removed one at a time, and a named test failed
each time. That includes the grant mod's spellings, which a test reads
from the module. Two earlier pieces made no test fail when removed. Both
were deleted, because each guarded nothing the rest of the gate misses:
- a mark that held a heredoc body as unreadable beside a process
  substitution, whose commands the gate already reads;
- doubt carried from what a printed body expands. Its only source was
  printf given a bracketed word, which bash does not evaluate.

## Consequences

- Some commands that merge nothing are now refused when they mention a
  merge, and "mergeable" counts, as in `gh pr view 5 --json mergeable`:
  - arithmetic that names a variable, as `x=1; echo $((x))`;
  - a function defined in the same command;
  - `gh api graphql --input /dev/fd/0`;
  - a GraphQL request read from an unquoted heredoc holding `$`, a
    backquote or a backslash, such as one asking for a pull request's
    `mergeable` with `$N` in the body. Quoting the delimiter lets the gate
    read it.
- A merge whose number or flags bash computes, as
  `for i in 5; do gh pr merge $i; done`, is refused and cannot be granted.
  Writing the number lets the gate read it.
- A merge run through `sudo`, `ssh` or another program, or in a command
  that sets what gh reads, as `GH_REPO=o/r gh pr merge 5`, needs its head
  named and the operator's grant, even in a private repository or for the
  account's own pull request.
- A merge after `pushd`, `popd` or `cd "$DIR"` must name its repository.
- The change is in `swarm-terminal-host` and the grant mod it writes. It
  reaches a worker only after the worker engine update, which stops running
  sessions.
