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

A review of this decision's first draft found more, which that draft passed
or, for the folder, checked against the wrong pull request:
- a shell reading a script the gate cannot see, as `bash <(...)`,
  `source <(...)` or `echo '...' | bash`, which that draft let through,
  though 1.25.0 and `main` refuse all but the last;
- a `cd` inside a condition, a loop or a coprocess, which that draft, now
  reading reserved words, carried to the commands after it where bash may
  not run it;
- a `cd` after `&&`, in the background, a subshell or a pipeline, which
  that draft carried as 1.25.0 does, and one bash searches through
  `CDPATH` or runs through `eval`, which none of them followed;
- `case` forms it still misread, `env -vS'...'`, `su -c`, `script -qc`,
  `flock -c` and `rbash -c`, code run by `python3 -c` or `perl -e`;
- curl's `-T` sending a `PUT`, and its abbreviated flags, as `--data-r` or
  `--conf`;
- `gh pr mer${z}ge 5`, which bash reads as a merge;
- a `PATH` set by `read -r PATH` or through `declare -n`.

## Decision

- **A merge is looked for as bash may spell it.** The grant mod and the gate
  each look for "merge" or `enqueuePullRequest`, and for `gh` or `curl`, in
  four spellings of the command:
  - as written;
  - without its quotes and backslashes, a `$` before a quote, and a
    backslash and newline;
  - the same, after reading each `\xHH`, `\NNN`, `\uHHHH` and `\UHHHHHHHH`
    as the letter it names, as `$'...'`, printf and JSON do;
  - that, without each expansion bash may make empty: `${...}`, a `$(...)`
    with no parenthesis inside, a backquoted command, `$` and a name, and
    `$` and a digit or one of bash's own parameters. So `gh pr mer${z}ge 5`
    and ``mer` `ge`` are found.

  A test runs the grant mod's own module under node, so each of these is
  checked as the mod runs it, not as text the module holds.
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
  - a shell reading a script the gate cannot see: a script named by a word
    bash computes, as `bash <(...)`, `source <(...)` and `. <(...)` are, or
    standard input that is not a heredoc it reads, as `bash < <(...)`,
    `cat <(...) | bash` and `echo '...' | bash` are. The shells are `bash`,
    `sh`, `zsh`, `dash`, `ksh`, `mksh`, `rbash` and `ash`. A script named as
    a file is not read, as before;
  - a program that runs code in a language of its own, Python, Perl, Ruby,
    Node, PHP or awk, given a word or a heredoc that spells a merge and gh
    or curl, as `python3 -c 'import os; os.system("gh pr merge 5")'` or
    `python3 <<'EOF'` with the same code, or code from a word bash computes
    or from input the gate cannot see. Given only flags, one of them
    `--version` or `--help`, it prints about itself and exits, so input it
    is given counts only if it spells a merge;
  - for gh, a computed word up to its command, and for `gh pr merge` or
    `gh api`, one that may split or is not a whole word. Two are read
    instead. The value of an output flag, as `--jq "$Q"`, only shapes what
    gh prints. A quoted endpoint that is `gh api`'s only operand, as
    `gh api "$URL"`, may be any endpoint: a request through it may be
    GraphQL, and a `PUT` through it is held as a merge of any pull request;
  - for curl, a computed word that may split, or that starts with a part
    bash computes and so may be a flag such as `-K`, unless it is the value
    of `-H`, `-u`, `-A`, `-o` or `-w`. A quoted address with a written start,
    as `"https://api.github.com/$P"`, or one given to `--url`, is read as
    any address: a `PUT` to it is held as a merge of any pull request, and
    a body the gate cannot see sent to it is held as a request it cannot
    read;
  - for `sudo`, `ssh` and the other programs below, a computed word;
  - in double quotes, `"$@"`, `"${@}"` and `"${a[@]}"`, which are any
    number of words, so in a merge they may be any flags. `"$*"`,
    `"${a[2]}"` and `"${#a[@]}"` are one word.

  In a heredoc body that is only printed (ADR 0188), none of these runs, so
  the body does not count.
- **Reserved words and wrappers are read through.** `{`, `!`, `if`, `then`,
  `elif`, `else`, `while`, `until`, `do`, `time` and `time -p`,
  `coproc NAME {`, `command`, `builtin`, `exec` and `exec -a`, `nohup`,
  `nice`, `stdbuf`, `timeout` and `env` each come before the program. A
  folder that `env -C` or `env --chdir` names is the merge's folder, and
  `cd` under a wrapper moves nothing after it. `env -S` runs its string,
  also at the end of a group of `env`'s letters, as `env -vS'...'`; a letter
  `env` does not know leaves the command held.
- **A runner given commands as one string is read.** The string after
  `su -c`, `runuser -c`, `flock -c`, `script -c` and `--command`, and the
  words after `ssh`'s host or `watch`'s flags, are read as commands of the
  same command, and a merge in them is held, as below.
- **A `case` is read as bash reads it.** Its subject and `in` are words,
  each pattern up to its `)` is no command, `|` separates patterns and a
  `(` may open one, and `;;`, `;&` and `;;&` end a clause. `esac` closes the
  case only where bash reads it as a word of its own. So a `)` in a
  pattern, inside a substitution or not, as in `$(case x in x) ...;; esac)`,
  `$(case x in x|esac) ...;; esac)` or `$(time -p case ...)`, ends neither
  the substitution nor the case.
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
  standard input there is held as one the gate cannot see. A `PATH` change
  is also a word `PATH` given to a command, as `read -r PATH`, or a value
  `PATH`, as `declare -n p=PATH`.
- **A process substitution's commands are read.** `<(...)` and `>(...)` run
  as a word, after digits as `2>(...)`, or as a redirection's file.
- **Every redirection bash reads is one.** `&>`, `>|` and `>&file` are read
  as redirections of the command.
- **curl's flags are read as curl reads them.** A group of one-letter flags
  ends at the first that takes a value, which takes the rest of the word or
  the next. Values may be attached. curl takes any unambiguous start of a
  long flag for the flag, so a strict start of one that sends a body or
  sets the method, as `--data-r`, `--js` or `--upload-f`, is held as a
  request the gate cannot see, and `--conf` is read as `--config`. `-T`
  sends with `PUT` unless `-X` names another method. These send a body the
  gate cannot see, so a request with one is held when it may be GraphQL:
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
  named. So does a `cd` that may not run in this shell, or runs in a shell
  of its own:
  - in an `if`, `while`, `until`, `for`, `select`, `{ ... }` or `case`,
    the condition included, and in a coprocess's compound;
  - after `&&` or `||`, to the end of that list;
  - in parentheses, in a list run in the background, in a pipeline of more
    than one command, and in a substitution;
  - in text `eval` runs, and after `source` or `.`, whose script may move
    the shell;
  - a `cd` to a folder that starts with none of `/`, `.` and `~`, when
    `CDPATH` is set in the gate's environment or named in the command,
    since bash searches it first.

  A `cd` run by `coproc` alone, as `coproc cd own`, moves nothing after it.
  A `cd` before a condition, as in `cd own; if ...; fi; gh pr merge 5`, is
  followed as before.

## What this does not cover

- **A merge spelled only through an expansion whose text never spells it.**
  A brace list or sequence, as `{mer,}ge` or `{l..n}erge`, a glob matching a
  file of that name, a curl URL pattern, as `.../pulls/5/mer{ge,}`, an
  expansion that supplies letters, as `mer${z:-g}e` or `mer$(printf g)e`,
  or a variable set by an earlier command, in a command that otherwise
  names no merge. The grant mod never sends it to the gate, so the gate
  cannot hold it. The gate does hold `curl -X PUT .../pulls/5/mer{ge,}`
  when it is sent. Whether the mod should send every gh or curl command is
  an open product question (`docs/09-open-questions.md`).
- **Code in another language that never spells the merge**, as Python
  running `'gh pr mer' + 'ge 5'`. The gate does not read Python, Perl or
  the others; it refuses their code only when a word spells a merge and gh
  or curl, or when it cannot see the code.
- **HTTP clients other than gh and curl**, as
  `wget --method=PUT .../pulls/5/merge`, `httpie`, or a library call from a
  program. The gate reads only gh's and curl's requests.
- **Merging a branch rather than a pull request.** GraphQL `mergeBranch`,
  the branch merges API, `gh api -X POST repos/o/r/merges` and the same sent
  by curl, merge one branch into another as a push does. They are still to
  be fixed separately, as ADR 0188 listed the REST one, and ADR 0179 names
  a push.
- **A config written before the command, or in a way the gate does not
  read**: gh's config, a git config file given with `--file`, `~/.curlrc`
  written by an earlier command, and `gh alias set NAME -` reading its
  expansion from standard input.
- **A `CDPATH` the gate does not see.** The gate reads `CDPATH` from its own
  environment and from the command. One the worker's shell sets from a
  profile, and which the worker's own process does not carry, is not seen.
  A relative `cd` there is followed to the folder bash would not use, and
  a merge after it is checked against that folder's repository.
- **What the gate now holds instead of reading.** `gh api graphql --input
  /dev/fd/0` is held as a request read from a file it cannot see, not read
  as standard input. A function, `gh $(echo ...)`, a shell reading a script
  from `<(...)` or a pipe, and code in another language from a word bash
  computes, are not followed: the command is refused.
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

A test writes the grant mod as the engine does and runs its module under
node, which strips the types itself, with a stand-in for the engine's
`tool.check` hook. It checks that the module sends seventeen spelled forms
to the gate and not five ordinary commands. `main`'s module sends none of
the quoted forms, and this decision's first draft misses `mer${z}ge`. CI's
rust job installs node 24 for it; it ran here under node 24.19.0.

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
| `ssh host 'gh pr merge 5'`, `su -c 'gh pr merge 5' w`, limits of the first draft | pass | pass | refuse |
| Limit: `gh alias set m - <<'B'` then `gh m` | pass | pass | pass |
| Limit: `curl -X PUT .../pulls/5/mer{ge,}`, which no grant mod sends | pass | pass | refuse |
| Limit: the branch merges API, `gh api -X POST repos/o/r/merges` | pass | pass | pass |
| Cost: `gh api graphql --input - --jq ...mergeable` given an unquoted heredoc with `$N` in its query | pass | pass | refuse |
| Cost: `gh api graphql --input /dev/fd/0 <<'B'` with a plain query | pass | refuse | refuse |
| Cost: `x=1; echo $((x)) && gh pr view 5 --json mergeable`, or a function defined before it | pass | pass | refuse |
| Twenty-four ordinary commands, among them the same request with the delimiter quoted and `$n` as a GraphQL variable, `gh pr view 5 --json mergeable`, `timeout 60 cargo test merge_gate`, `diff <(git show HEAD:a) a`, `echo "$(case x in x) echo hi;; esac)"`, `export PATH=$HOME/.cargo/bin:$PATH && cargo build`, `args=()`, `printf '%s' 'a[1]'`, `GH_HOST=x gh pr view 5`, `nice -n 5 cargo build` and `curl --variable` | pass | pass | pass |

### The review of the first draft

Seventy-three more commands ran the same way on four builds: 1.25.0,
`main`, the first draft at `ae3059c7`, and this change.

| Command | 1.25.0 | main | First draft | This change |
| --- | --- | --- | --- | --- |
| `bash <(echo gh pr merge 5)`, `source <(...)`, `. <(...)`, `bash < <(...)`, `cat <(...) \| bash`, or `bash <(echo gh api -X PUT .../pulls/5/merge)` | refuse | refuse | pass | refuse |
| `echo 'gh pr merge 5' \| bash` | pass | pass | pass | refuse |
| `echo "$( (case x in x) true;; esac) ; gh pr merge 5 )"`, `$(case x in x\|esac) ...` or `$(time -p case ...)` | pass | pass | pass | refuse |
| `env -vS'gh pr merge 5'` | pass | pass | pass | refuse |
| curl `-T /dev/null` or `-sT` to `.../pulls/5/merge`, `--upload-f`, a merge mutation sent with `--data-r`, `--js` or `--data-b`, or `--conf m.cfg` | pass | pass | pass | refuse |
| A merge run by `python3 -c`, `perl -e`, `script -qc`, `flock -c`, `rbash -c`, `ssh host '...'`, `ssh echo '...'`, `su w -c '...'` or `watch --interval 5 '...'` | pass | pass | pass | refuse |
| `gh pr mer${z}ge 5` | pass | pass | pass | refuse |
| `g${z}h pr merge 5` | pass | pass | refuse | refuse |
| `gh pr mer$(true)ge 5`, `mer$( )ge` or ``mer` `ge`` | pass | refuse | refuse | refuse |
| `read -r PATH <<< /tmp/x` or `declare -n p=PATH; p=/tmp/x`, then `cat <<'C' \| gh api graphql --input -` | pass | pass | pass | refuse |
| `gh pr merge 5 --subject "$@"`, or `"${a[@]}"` | refuse | refuse | refuse | refuse |
| `gh api -X PUT "repos/o/r/pulls/$N/merge"`, a merge mutation to `gh api "$URL"`, or `curl -X PUT "https://api.github.com/$P"` | pass | pass | refuse | refuse |
| Cost narrowed: `gh api "repos/$REPO/pulls/5" --jq .merged`, `gh api repos/o/r/pulls/"$N"`, `gh api "$URL" --jq .mergeable`, `curl -s "https://api.github.com/repos/o/r/pulls/$N" \| jq .mergeable`, or a `"${prs[@]}"` loop asking for `mergeable` | pass | pass | refuse | pass |
| Cost: `gh api repos/$REPO/pulls/5 --jq .mergeable`, the same in a loop over `$pr`, `-F n=$N` beside a `mergeable` query, curl to `.../pulls/$N` unquoted, `[[ $n -gt 0 ]]` or a function and `trap` before `gh pr view 5 --json mergeable`, or `curl -s "$URL" \| jq .mergeable` | pass | pass | refuse | refuse |
| Cost: `node -v` or `python3 -V` before `gh pr view 5 --json mergeable` | pass | pass | pass | refuse |
| Twelve ordinary commands, among them `python3 --version` or `node --help` before `gh pr view 5 --json mergeable`, `echo 'gh pr view 5' \| bash`, `script -qc 'gh pr view 5' /dev/null`, `case x in merge) gh pr view 5;; esac` and `curl -X POST -T m.json .../pulls/5/merge` | pass | pass | pass | pass |
| Limits: `wget --method=PUT`, GraphQL `mergeBranch`, `gh api -X POST repos/o/r/merges`, Python joining `'mer' + 'ge'`, `mer${z:-g}e` and `mer$(printf g)e` | pass | pass | pass | pass |
| `curl -X PUT .../pulls/5/mer{ge,}`, which no grant mod sends | pass | pass | refuse | refuse |

The 159 commands above answer the same on this change as on the first
draft, apart from `ssh host '...'` and `su -c '...'`, which now refuse.

A gh that only logs the folder it runs in and then fails stood first on
PATH for twenty-one folder forms:
- On 1.25.0, `main` and the first draft, gh was asked about `other` after
  `false && cd other`, `cd other &`, `(cd other)` and `cd other | true`,
  where bash stays where it was. It was asked about the first folder after
  `eval 'cd other'`, where bash moves.
- The first draft also asked about `other` after
  `if false; then cd other; fi`, `while false`, `for d in;`, an `else` not
  taken and `coproc { cd other; }`, where 1.25.0 and `main` asked about the
  first folder: the regression the review found.
- With `CDPATH` set, all three asked about `start/own`, where bash went to
  the `CDPATH` folder's `own`.

This change asks gh in no folder for each of those: it leaves the folder
untold and refuses. It still follows `cd other && ...`, `cd other; ...`,
`cd other || exit 1; ...`, `cd ./own` under `CDPATH`, and a `cd` after a
`case` or after `source`. `coproc cd other` moves nothing after it. Bash
5.2.21 was run on the `CDPATH` form and on the `case` forms.

curl 8.5.0 was run against a stub server on 127.0.0.1:
- `-T` and `-sT` sent `PUT`;
- `-T` with `-X POST`, before or after, sent `POST`;
- `--upload-f` sent `PUT`, and `--data-r`, `--data-b` and `--js` sent a
  body;
- `--conf` read the config and sent its request;
- `--req` and `--reque` are ambiguous and exit 2, as does `--request=PUT`;
- `--sil` and a `--disable` that is not first sent a plain `GET`.

Python 3, Perl, Node, PHP and gawk were each given code on standard input
with only `--version` or `--help`. None ran it, and each ran it with no
flag. Ruby is not installed here, so for Ruby the rule rests on its manual.

A check of this change before it was committed found one more. Python,
Perl or Node reading its code from a heredoc, as `python3 <<'EOF'` running
`os.system("gh pr merge 5")`, `perl - <<'EOF'` or `cat <<'EOF' | node`,
passed on 1.25.0, `main` and the first draft, since the gate read the body
only as shell commands. It is refused now. `python3 - <<'EOF'` with
`gh pr merge 5` as its body, which ADR 0188 held as a merge, is refused
with it: Python reads the body as code, which the gate does not read.

### Tests and ablations

Twenty-eight tests are new, and each passes on this change: twenty-seven
for the gate and the grant mod's. The seventeen for the gate from the first
draft fail on `main`'s gate. The ten from this review fail on `main`'s gate
and on the first draft's. The grant mod's fails with `main`'s module and
with the first draft's.

On the final source, 188 pieces of the change were removed one at a time,
and a named test failed each time. These are the first draft's 72, moved
to where this review left their code, and 116 for this review's pieces.
The grant mod's are among them, since its test runs the module. The source
was restored byte for byte after each.

Pieces that no test could tell from their absence were deleted, not kept:
- in the first draft, a mark that held a heredoc body as unreadable beside
  a process substitution, whose commands the gate already reads; and doubt
  carried from what a printed body expands, whose only source was printf
  given a bracketed word, which bash does not evaluate;
- in this review:
  - the empty `$()` and pair of backquotes that the grant mod and the gate
    took out, which taking out each expansion that may be empty covers;
  - an arm for `|` between `case` patterns, whose words are dropped
    anyway;
  - `watch`'s `-n` and `--interval` values, a number, which is read as a
    program whose words are read;
  - a check that gh's open endpoint does not start with `-`, which no
    operand does;
  - `${!prefix@}` as many words, a name read as a value, which the gate
    already holds;
  - a pass for `-c` given no code, and a reset of `&&` at a clause's end,
    which the `case` already makes uncertain.

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
- **A variable in a gh api or curl word refuses an ordinary command once
  the command says merge, mergeable or merged.** The gate cannot tell what
  such a word becomes, and an unquoted one may split into several words,
  any of them a flag such as `-X PUT`. So these are refused, though they
  only read:
  - `gh api repos/$REPO/pulls/5 --jq .mergeable`, and the same in a loop
    over `$pr`;
  - `gh api graphql -F n=$N -f query='... mergeable ...'`;
  - `curl -s https://api.github.com/repos/o/r/pulls/$N | jq .mergeable`;
  - `curl -s "$URL" | jq .mergeable`, whose address, starting with what
    bash computes, may be a flag such as `-K`.

  Where it could be narrowed without passing a merge, it was. Quoting the
  variable makes it one word, so these pass: `gh api "repos/$REPO/pulls/5"`,
  `gh api repos/o/r/pulls/"$N"`, `gh api "$URL"` as the only operand,
  `--jq "$Q"`, a quoted curl address with a written start, as
  `"https://api.github.com/repos/o/r/pulls/$N"`, `--url "$U"`, and
  `"${prs[@]}"` in a `for` loop's list. Each of these is held instead when
  the request is a `PUT`, or sends a body the gate cannot see.
- A command that mentions a merge is refused when it holds a shell reading
  a script the gate cannot see, as `cat x | bash`, or a program in another
  language reading code it cannot see, as `python3 "$S"`, `node -v` or
  `python3 -V`, which read standard input as far as the gate can tell.
  `--version` and `--help` pass. A heredoc such a program reads as data
  is refused too when it spells a merge and gh or curl, as
  `awk '{print $1}' <<'EOF'` or `python3 m.py <<'EOF'` with
  `gh pr merge 5` as its body, which `main` held as a merge to grant.
- A `cd` that may not run in this shell, as `true && cd crates` or
  `(cd crates)`, leaves the folder untold. A merge after it must name its
  repository with `--repo`, or the `cd` must come first, as
  `cd crates; ...`, or on its own.
- A merge run through `sudo`, `ssh` or another program, or in a command
  that sets what gh reads, as `GH_REPO=o/r gh pr merge 5`, needs its head
  named and the operator's grant, even in a private repository or for the
  account's own pull request.
- A merge after `pushd`, `popd` or `cd "$DIR"` must name its repository.
- The change is in `swarm-terminal-host` and the grant mod it writes. It
  reaches a worker only after the worker engine update, which stops running
  sessions.
