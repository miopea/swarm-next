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

A review of the second draft, at `36be3362`, found two regressions. The
draft took a reserved word for one wherever it stood after another word:
- given as an argument, as in `echo if case x in; gh pr merge 5`, it let
  the `case` after it open, and every word after that was dropped as a
  pattern. `main` refuses these; that draft passed them;
- given as an argument, as in `if true; then echo then fi; else cd own; fi`,
  it closed the `if` there. A `cd` in the `else` was then carried to the
  merge after it, which bash runs in the first folder.

That review found more merges that `main` also passes:
- `bash -c $'\tgh pr merge 5'` and `eval $'\ngh pr merge 5'`, where a tab
  or newline from `$'...'` stands before gh. No grant mod sent them;
- a program that runs a word, or what it reads, as a command: sed's `1e`
  and `s///e`, m4's `syscmd`, tclsh's `exec`, expect's `spawn`,
  `vim -es '!...'`, `make -f -`, `sg -c`, tmux and `git rebase -x`;
- xargs giving gh its words, as `echo merge 5 | xargs gh pr`, or a shell
  its text, as `xargs -I{} bash -c {}`;
- an address that spells `m` as `%6d`, as `.../pulls/5/%6derge`.

And merges that `main` checks against the wrong folder's repository:
- after `cd own x`, which bash refuses, so it moves nowhere;
- after a relative `cd` under a `CDPATH` whose name bash computes, as
  `declare "CD${z}PATH=../x"`.

Fixing these turned up three more, which `main` and the second draft also
miss:
- shell text that bash computes in part, as `bash -c "gh pr view $N"` with
  `N='5; gh pr merge 5'`, which they pass. The shell parses again what bash
  put there;
- a merge held in a variable and printed into a program that runs what it
  reads, as `echo "$c" | sudo bash` or `tclsh <<< "$c"`, which they pass;
- a `CDPATH` exported before `bash -c 'cd own && ...'` or `eval`, which
  that shell inherits, and which they check against the first folder's
  `own`.

A review of the third draft, at `d27e7a74`, found three more regressions:
two in the gate, where `main` refuses or checks against the right folder,
and one in the grant mod, against the second draft's:
- after `time --`, `time -p --` or `coproc NAME`, that draft read no
  command position, so a `[[` there opened nothing. In
  `time -- [[ x && case == in ]]; gh pr merge 5; case y in y) :;; esac` the
  `case` inside the `[[` opened one, the merge was dropped as a pattern,
  and the last `esac` closed it. The same held for gh api's `PUT`, curl's,
  a GraphQL merge and `--repo`;
- after `time --`, an `if` or `while` opened nothing, so a `cd` in one not
  taken, as `time -- if false; then cd own; fi; gh pr merge 5`, was carried
  to the merge;
- the grant mod read an escape outside `$'...'`, as the `\e` in
  `gh pr $'\x6d'\erge 5`, as the letter bash reads only inside one, so it
  sent none of `gh pr $'\x6d'\erge 5`, `mer$'\x67'\e`, `$'\x6d'e\rge`, the
  same in gh api's path, or `$'\x63'u\rl -X PUT ...` to the gate.

That review found more that `main` also passes:
- a closer inside an array, as `if false; then a=(fi); cd own; fi`, which
  closed the compound, so the `cd` after it was carried to the merge;
- in the grant mod, an octal escape past `\377`, as `$'\547'`, which bash
  reads as its low byte, `g`, and `\cX`, as `$'\cIgh pr merge 5'`;
- `echo merge 5 | busybox xargs gh pr`, where busybox runs xargs.

A review of the fourth draft, at `fbce7ade`, found one regression, against
the second draft's grant mod. A `$'` that begins no string inside the
merge's own word, as in `gh pr $'\x6d'$(: "$'")\erge 5`, put that draft's
readings from each place a string may begin out of step. Its module sent
neither that form nor five like it to the gate, and its gate passed
`gh pr mer$'\x67'${z:+"$'"}\e 5`. It also measured that draft's module at
6 to 7 seconds on a 1.9 MB command holding 32 `$'` strings, against 0.4
before. That review found three more that `main` also gets wrong, each
against this record's own text:
- a subscript bash reads whole, as in `a[ 0 ]=(fi)` or `a[0; fi ]=1`, which
  that draft split at its spaces and `;`, so a closer in it closed the
  compound and a `cd` after it was carried to the merge;
- `false && a[0; case x in ]=1; $g pr $m 5; case y in y) :;; esac`, where
  the `case` in the subscript opened one, the merge was dropped as a
  pattern, and the last `esac` closed it;
- xargs given its words behind `ionice`, `taskset`, `chrt`, `flock`,
  `setsid` or `sudo`, as `echo merge 5 | ionice xargs gh pr`, where that
  draft dropped what xargs left unread.

Checking this change before it was sent found two more that `main` also
gets wrong:
- a `${...}` or `$[...]` read a character at a time. In double quotes a
  quote inside one, as in `"${x:-"; fi; cd own; "}"`, ended the quotes, so
  the `fi` closed the compound and the `cd` was carried to the merge;
- a `%HH` escape split by what bash takes out, as in `.../pulls/5/%6''derge`,
  `%6\Derge` or `%$'\x36'derge`. The grant mod read `%HH` escapes before
  taking quotes out, so of seven such forms the fourth draft's sent only
  the last to the gate and `main`'s sent none, and bash runs each as a
  merge. `main`'s gate passes each.

## Decision

- **A merge is looked for as bash may spell it.** The grant mod and the gate
  each look for "merge" or `enqueuePullRequest`, and for `gh` or `curl`, in
  six spellings of the command. Each but the first is without its quotes
  and backslashes, a `$` before a quote, and a backslash and newline:
  - as written, and without those;
  - without those, with each run of `%HH` escapes read as the text it
    makes, as a server may read an address once bash has made it. So
    `.../pulls/5/%6derge` is found, and so are `%6''derge` and `m%6\5rge`,
    where bash takes out what splits the escape;
  - with each escape that names a letter by its number read, wherever it
    stands: `\xHH`, `\NNN`, which keeps its low byte, so `\547` is `g`,
    `\uHHHH` and `\UHHHHHHHH`. Each other escape is kept, so a backslash
    and a letter is that letter once its backslash is taken out, as bash
    reads it outside `$'...'`. So `gh pr $'\x6d'\erge 5`, which bash runs
    as `gh pr merge 5`, is found. This reading is looked at twice: with its
    `%HH` escapes read, so `%$'\x36'derge` is found, and without each
    expansion bash may make empty, as below;
  - with every escape read, as printf and JSON read them, the one-letter
    ones such as `\t` and `\n` among them, without each expansion bash may
    make empty. So `printf '\tgh pr merge 5' | bash` is found. Text that
    printf supplies from its arguments, through `%s` or `%b`, is not: see
    "What this does not cover".

  Bash makes a letter only from a letter, a backslash and a letter, or a
  number in `$'...'`. So the numbered reading finds each merge bash spells
  wherever a `$'...'` string begins, and whether a `$'` begins one or not:
  one inside quotes, after a backslash or in a comment begins none, as in
  `grep -c 'x$' notes.md`, and the text alone does not tell it from one
  that does. A one-letter escape or `\cX` in `$'...'` makes a tab, a
  newline or another byte that is no letter, which may stand before `gh`
  or `curl` where bash splits a word. So in a command that holds a `$'`,
  gh or curl counts wherever it stands, even inside a word. Each spelling
  is one pass over the text.

  The expansions bash may make empty are `${...}`, a `$(...)` with no
  parenthesis inside, a backquoted command, `$` and a name, and `$` and a
  digit or one of bash's own parameters, so `gh pr mer${z}ge 5` and
  ``mer` `ge`` are found. For each of the last four spellings, a test holds
  a command that only it finds, as `gh pr me${x}rg\e 5` for the numbered
  reading without expansions.

  These spellings decide only whether a command mentions a merge and gh or
  curl. The gate reads the command's words as bash does, a `$'...'` string
  among them, as below. The grant mod reads the same six, and does not read
  `\cX`, which makes no letter. A test runs the grant mod's own module
  under node, so each of these is checked as the mod runs it, not as text
  the module holds.
- **A command whose runs cannot be told from its text is not read.** Where
  the command mentions a merge, it is refused as unreadable, and never
  grantable, when it holds:
  - a value bash evaluates as arithmetic or as a name: `$((x))`, `((x))`,
    `$[x]`, `${a[x]}`, `${!x}`, `${s:x}`, `let`, `[[` with `-eq` and the
    like or `-v`, `declare -i`, `a[x]=`, or a bracketed name given to
    `read`, `unset`, `mapfile`, `readarray`, `getopts`, `wait`, `test`, `[`
    or `printf -v`. `${x}`, `${#x}`, `${x:-y}`, `${x%.md}` and `${x^^}` are
    read as they are, and so is arithmetic that names no variable;
  - a function defined, as `f() {` or `function f {`. `a=()` is an array,
    and any other word before `()` names a function, as bash reads `=()`
    and `"a"=()`;
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
  - shell text that bash computes in part, which the shell parses again with
    what bash put there, so `N='5; gh pr merge 5'; bash -c "gh pr view $N"`
    runs a merge. That is the text after a shell's `-c`, `eval`'s words,
    a runner's string as below, `trap`'s, an alias's value, `gh alias set`'s
    expansion, git's `-c` value, `env -S`'s string, and the value of a
    variable that names a program. `eval "$(...)"` is one, whatever the
    substitution prints;
  - a program the gate does not know, with no command found among its
    words, that may run what it is given as a command:
    - given a word that spells a merge and gh or curl, as
      `sed -n '1e gh pr merge 5'`, `expect -c 'spawn gh pr merge 5'`,
      `vim -es -c '!gh pr merge 5'`, `sg users -c '...'` or
      `tmux new -d '...'`;
    - reading, from a pipe or a redirection, input the gate cannot see, in
      a command where a word spells a merge and gh or curl, or after echo or
      printf in its pipe printing words that spell one together, as
      `echo 'syscmd(gh pr merge 5)' | m4`, `c='...'; echo "$c" | sudo bash`
      or `tclsh <<< "$c"`.

    git's commands other than those that store, send or show what they are
    given, as `rebase -x`, `bisect run` and `submodule foreach`, count as
    such a program: a merge among their words is held, as below. grep,
    `egrep` and `fgrep` run nothing they are given;
  - xargs's command, whose words it gives from its input or a file: without
    `-I`, words after the command's own, and with `-I`, `-i` or `--replace`,
    a word in place of each that holds its string. They are words bash
    computes, so `echo merge 5 | xargs gh pr` and
    `xargs -I{} bash -c {}` are not read. Shell text it runs as written, as
    `xargs -0 bash -c 'gh pr merge 5'`, is read. An option xargs has that
    the gate does not know, or one bash computes, leaves it unread;
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
  `elif`, `else`, `while`, `until`, `do`, `time` with `-p`, `--`, both or
  neither, `coproc NAME {`, `command`, `builtin`, `exec` and `exec -a`,
  `nohup`, `nice`, `stdbuf`, `timeout` and `env` each come before the
  program, and so do `busybox` and `toybox`, which run the applet named
  after them: `busybox xargs` is xargs, and `busybox sh -c` a shell. A
  folder that `env -C` or `env --chdir` names is the merge's folder, and
  `cd` under a wrapper moves nothing after it. `env -S` runs its string,
  also at the end of a group of `env`'s letters, as `env -vS'...'`; a letter
  `env` does not know leaves the command held.
- **A word is reserved only where bash reads a command.** That is the
  text's start, after a list's or a pipeline's end, after a reserved word
  bash reads as one, after `time`'s options, `-p`, `--` or `-p --`, and
  after a compound's close, `fi`, `done`, `}`, `esac` or `]]`. A `-p` after
  `time --` or `time -p` is a program's name, as bash runs it. `do` is
  reserved after `for` or `select` and its name. After `coproc` and a name,
  only a compound's opener is: `{`, `if`, `while`, `until`, `for`, `select`,
  `case` or `[[`, and `((` is arithmetic wherever it stands. A reserved word
  given as an argument, as `echo if` or `echo then fi`, is a word, and opens
  or closes nothing. Inside `[[ ... ]]` no word is reserved.
- **A runner given commands as one string is read.** The string after
  `su -c`, `runuser -c`, `flock -c`, `script -c` and `--command`, and the
  words after `ssh`'s host or `watch`'s flags, are read as commands of the
  same command, and a merge in them is held, as below. A string bash
  computes in part is not read, as above.
- **A `case` is read where bash reads one, as bash reads it.** It opens only
  where a word is reserved, as above. Its subject and `in` are words, each
  pattern up to its `)` is no command, `|` separates patterns and a `(` may
  open one, and `;;`, `;&` and `;;&` end a clause. `esac` closes the case
  only where bash reads it as a word of its own. So a `)` in a pattern,
  inside a substitution or not, as in `$(case x in x) ...;; esac)`,
  `$(case x in x|esac) ...;; esac)` or `$(time -p case ...)`, ends neither
  the substitution nor the case. A pattern that names gh, curl or merge as a
  word of its own may be a command read wrongly as a pattern, so a command
  that mentions a merge and holds one is not read: `case x in merge)` there
  is refused, and `MERGED)` or `merged)` is not such a word.
- **An array's words are words.** `name=(...)`, `name+=(...)` and
  `name[...]=(...)`, with the name as written, assign an array, and its
  words up to the `)` that ends it are part of the assignment's word. None
  is reserved, so `a=(fi)`, `a=(done)`, `a=(})` and `a=(esac)` close
  nothing. A `#` after a space starts a comment to the line's end, a quoted
  `)` ends nothing, and a substitution or a process substitution among them
  runs and is read. A `(`, `;`, `|`, `&` or a redirection in one is a syntax
  error, and the text is not read. Nor is a newline in one while a heredoc
  waits for its body: bash 5.2.21 reads that body after the array, to the
  text's end. A quoted name, as `"a"=(x)`, or one bash computes, assigns
  nothing, and bash refuses its `(`.
- **A subscript is read to the bracket that matches it.** Where bash may
  read an assignment, it reads `name[` to the `]` that matches it, with
  spaces, `;`, newlines and reserved words among it. A `[` nests, a
  backslash takes the character after it, and a quote, a backquote,
  `$(...)`, `${...}`, `$[...]` and `$'...'` are read whole, so a `]` in one
  ends nothing. Bash reads one so when the word so far is a name as
  written, unquoted, and an assignment may stand there:
  - at a command's start, as above;
  - after another assignment, whether its value is quoted or computed;
  - after redirections that nothing but redirections came before.

  In an array, a word that begins with `[` is read the same way. So
  `a[ 0 ]=(fi)`, `a[0; fi ]=1` and `a[0; case x in ]=1` close and open
  nothing. Elsewhere bash splits it as any other word, and so does the
  gate: after a command's name, after a redirection that follows an
  assignment, in a `case` word, after `for`, inside `[[`, and after a
  quoted or computed name. A word such as `a[ 0 ]x=1` is no assignment,
  so a `b[` after it is split. A subscript is arithmetic, so a command that
  mentions a merge is not read past an assignment to one, as before.
- **A parameter is read to the brace that matches it.** An unquoted
  `${...}` or `$[...]` is read whole, with spaces and `;` among it. Inside
  double quotes, a `"`, a backquote, `$(...)` and `${...}` nest in one, and
  its `}` closes it. So in `echo "${x:-"; fi; cd own; "}"` nothing after
  the first quote ends the word, and a `$(...)` inside one runs and is
  read. A `'` nests only in a pattern or a change of case (`#`, `%`, `/`,
  `^`, `,`), where bash runs nothing inside it. In the word of a default,
  an alternative, an assigned value or an error message (`-`, `+`, `=`,
  `?`, with a `:` or without), inside double quotes or inside such a word
  of a `${...}` that is, bash 5.2.21 reads a `'` as a letter and runs a
  substitution between two, a `$'...'` too; in posix mode and in dash the
  first `}` there ends the `${...}`, even between two `'`. Which applies
  cannot be told from the text, so a command that mentions a merge and
  holds a `'` or a `$'` there is refused. Text nested more than four deep
  in `${...}`, `$[...]` and subscripts is not read either.
- **Text that ends inside a `case`, a compound command or `[[` is not
  read.** Bash refuses it as a syntax error, after running the lines before
  the one left open. One that mentions a merge is refused as unreadable, as
  is a substitution that ends with one left open.
- **`$'...'` is the text bash makes of it.** Bash reads its escapes as it
  reads the command, so it is quoted text, not a word bash computes:
  `gh pr $'merge' 5` is read as a merge. Bash finds where the string ends
  first, reading a backslash and the character after it as a pair, and
  then reads its escapes. So `$'\c\\'` ends at its quote, and the command
  after it runs. A `\0` ends what it keeps, an escape bash does not name
  keeps its backslash, `\cX` is the control letter of X, and `\c\\` is one
  control letter, the control letter of a backslash. An octal escape past
  `\377` keeps its low byte, so `$'\547'` is `g`.
- **A here-string is a heredoc's body.** A here-string that bash computes no
  part of is what the command reads on standard input, and, as ADR 0188
  reads a heredoc's body, its text is read as commands too. So a merge in
  `tclsh <<< 'exec gh pr merge 5'` is found, and held as another program's.
  One that bash computes in part is input the gate cannot see.
- **A merge another program runs is held.** gh or curl run by `sudo`,
  `doas`, `runuser`, `su`, `watch`, `flock`, `ssh`, `setsid`, `chroot`,
  `nsenter`, `unshare` or `parallel`, or among the words of any other
  program the gate does not know, as `find . -exec`, may run as another
  account or on another machine. It is read, and held as a merge the gate
  cannot check, so it must name its head and needs the operator's grant.
  The words parallel fills in, from `:::` or from its input, are not read:
  see "What this does not cover".
  What the command among such a program's words leaves unread stays
  unread, as it is without the program, unless bash computes that
  command's name. That covers xargs's words, a word bash computes in a
  merge, and a shell reading input the gate cannot see. So
  `echo merge 5 | ionice xargs gh pr`, `m=merge; taskset 1 gh pr $m 5`
  and `curl ... | sudo bash` are refused. That command reads the command's
  input, so a heredoc given to `sudo -u w bash <<'EOF'` is read as before.
- **What gh reads from its environment is held.** A merge in a command that
  sets, exports, declares or unsets a variable gh or git reads to choose an
  account, host or repository, or runs gh under `env -i` or another `env`
  option the gate does not know, is held the same way. Those variables are
  `GH_*` and `GIT_*` other than ones that only change output or prompts,
  `GITHUB_*`, `HOME`, `PATH`, `XDG_CONFIG_HOME` and `CURL_HOME`.
- **A name bash computes may be any name.** A command that sets a variable
  whose name bash computes counts as setting each variable this decision
  names: `CDPATH`, `PATH` and those gh reads. That is an argument of
  `declare`, `typeset`, `local`, `export` or `readonly` that bash computes
  and that is no assignment to a name as written, as
  `export CD${z}PATH=../x`, any such argument with `-n`, the name
  `printf -v` sets, and one given to `read`, `mapfile`, `readarray` or
  `getopts`. `export FOO="$x"` sets `FOO` only.
- **An address is read with its `%HH` escapes read.** The merge API's path
  and GraphQL's are matched after each run of `%HH` escapes is read as the
  text it makes, as a server may read the path.
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
  plain folder is followed. `cd` is followed as bash's moves: past its
  flags, `-L`, `-P`, `-e` and `-@` alone or in a group, and a `--`, to its
  one folder, or home with none. Any other `pushd`, `popd`, a `cd` to a
  folder bash computes, and a `cd` given more than one folder or a flag it
  does not have, which bash refuses, leave the merge's folder untold, so its
  repository must be named. So does a `cd` that may not run in this shell,
  or runs in a shell of its own:
  - in an `if`, `while`, `until`, `for`, `select`, `{ ... }` or `case`,
    the condition included, and in a coprocess's compound. A compound is
    one only where its words are reserved, as above, so `echo then fi`
    inside an `if` neither opens nor closes one, and nor does a word of an
    array, as `a=(fi)`;
  - after `&&` or `||`, to the end of that list;
  - in parentheses, in a list run in the background, in a pipeline of more
    than one command, and in a substitution;
  - in text `eval` runs, and after `source` or `.`, whose script may move
    the shell;
  - a `cd` to a folder that starts with none of `/`, `.` and `~`, where
    `CDPATH` may be set, since bash searches it first. It may be set in the
    gate's own environment, by a word in the command that names it, by a
    name bash computes, as above, or around a shell's text the command
    runs, as `export CDPATH=../x; bash -c 'cd own && ...'`.

  A `cd` run by `coproc` alone, as `coproc cd own`, moves nothing after it.
  A `cd` before a condition, as in `cd own; if ...; fi; gh pr merge 5`, is
  followed as before.

## What this does not cover

- **A merge spelled only through an expansion whose text never spells it.**
  A brace list or sequence, as `{mer,}ge` or `{l..n}erge`, a glob matching a
  file of that name, a curl URL pattern, as `.../pulls/5/mer{ge,}`, an
  expansion that supplies letters, as `mer${z:-g}e` or `mer$(printf g)e`,
  a `%HH` escape that an expansion splits, as `.../pulls/5/%6${z}derge`,
  or a variable set by an earlier command, in a command that otherwise
  names no merge. The grant mod never sends it to the gate, so the gate
  cannot hold it. The gate does hold `curl -X PUT .../pulls/5/mer{ge,}`
  when it is sent. Whether the mod should send every gh or curl command is
  an open product question (`docs/09-open-questions.md`).
- **Code in another language that never spells the merge**, as Python
  running `'gh pr mer' + 'ge 5'`. The gate does not read Python, Perl or
  the others; it refuses their code only when a word spells a merge and gh
  or curl, or when it cannot see the code.
- **A program the gate does not know, running a merge that no word or input
  it sees spells.** One that builds the merge from parts, as m4 joining
  macros, or reads it from a file, as `m4 x.m4`, `sed -f s.sed` or a
  Makefile, or from a command whose output the gate does not follow, as
  `gh api ... | m4`, is not held. A script named as a file is not read, as
  before.
- **Whether GitHub reads `%6d` in a path as `m`.** The gate reads an
  address's `%HH` escapes as a server may, so `.../pulls/5/%6derge` is held
  as a merge of pull request 5. Whether GitHub's API decodes them in its
  path, and so would merge, was not tried: no request was sent.
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
  environment and from the command, as above. One the worker's shell sets
  from a profile, and which the worker's own process does not carry, is not
  seen.
  A relative `cd` there is followed to the folder bash would not use, and
  a merge after it is checked against that folder's repository.
- **What the gate now holds instead of reading.** `gh api graphql --input
  /dev/fd/0` is held as a request read from a file it cannot see, not read
  as standard input. A function, `gh $(echo ...)`, a shell reading a script
  from `<(...)` or a pipe, and code in another language from a word bash
  computes, are not followed: the command is refused. So is
  `eval "$(cat <<'EOF' ...)"`, which runs the body the gate reads, and
  which the second draft held as a merge to grant.
- **Four gaps the fifth review found, which every build has had or which
  earlier drafts brought, left open in this change:**
  - in an unquoted heredoc's body that reads as commands, what the body
    expands is not read again, so a substitution inside `'...'` there, as
    `cat <<E` with `'$(gh pr merge 5)'` or `x'$(gh pr merge 5)'y` in its
    body, passes. Bash runs it as the heredoc expands;
  - text given to `sh` or `dash` is read with bash's rules. Dash has no
    `$'...'`, so `$'\'` there is `$` and a quoted backslash, and a merge
    after it, as in `sh <<'E'` with `echo $'\' ; gh pr merge 5 ; echo '\'`,
    passes. The same goes for `sh -c` and `dash -c`;
  - text printf supplies from its arguments, as
    `printf '%sgh pr merge 5\n' '' | bash`, `printf 'g%sh pr merge 5\n' ''`
    or `printf '%bgh pr merge 5\n' '\t'`, piped to a shell. The gate refuses
    each when it sees it, but the grant mod does not send it;
  - the words GNU parallel fills in, as `parallel gh pr {} 5 ::: merge` or
    `echo merge | parallel gh pr {} 5`, pass.
- ADR 0188's other limits stand. The ones this decision fixes are listed in
  its Context.

## Evidence

Each of 159 commands, grouped below, ran through the gate's own entry
point, `swarm-terminal-host provider-merge`, on three builds:
- the installed 1.25.0 engine, from commit `be2c994d`;
- `main` before this change, at `8c26cffd`, which holds ADR 0188;
- this change.

An earlier draft of this record said gh was not on PATH for these runs.
That held for some: others kept the session's `HOME`, where gh is in
`/usr/bin` and logged in, so the gate's own gh calls, which only read who
opened a pull request and whether a repository is public, may have reached
GitHub, for `o/r`, the one repository the commands name. They ran from
`/srv`, which holds no repository, and no answer turned on them: in the
final runs for the third review, every probe ran with its environment
replaced and `HOME` pointing nowhere, so each gh call stopped before sending
anything, asking for `gh auth login`, and the 159 answered as before. A
merge the gate finds is refused as one it could not check. That is also why
a merge the gate reads but holds, as with `GH_REPO` or `pushd`, refuses on
all three: the unit tests show which reason each gives. Bash 5.2.21 was run
on the forms the rule rests on, with harmless commands in place of a merge:
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
| Eleven ordinary commands, among them `python3 --version` or `node --help` before `gh pr view 5 --json mergeable`, `echo 'gh pr view 5' \| bash`, `script -qc 'gh pr view 5' /dev/null` and `curl -X POST -T m.json .../pulls/5/merge` | pass | pass | pass | pass |
| Cost of the third review: `case x in merge) gh pr view 5;; esac`, held by its pattern rule | pass | pass | pass | refuse |
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

### The review of the second draft

Eighty-three more commands ran the same way on four builds: 1.25.0,
`main`, the second draft at `36be3362`, and this change.

| Command | 1.25.0 | main | Second draft | This change |
| --- | --- | --- | --- | --- |
| A reserved word given as an argument before `case x in`, then a merge: `echo if case x in; gh pr merge 5`, `echo do ...`, `echo time -p ...`, `printf '%s\n' if case x in; ...` and `: then case x in` with a merge on the next line; the first also before `gh api -X PUT .../pulls/5/merge`, curl's PUT to it, or a GraphQL merge mutation | refuse | refuse | pass | refuse |
| `echo hi; case x in` with a merge on the next line, which bash refuses as unfinished after running the first line | refuse | refuse | pass | refuse |
| `if true; then gh pr merge 5`, left open | pass | pass | refuse | refuse |
| `bash -c $'\tgh pr merge 5'`, `bash -c $'\x09gh pr merge 5'` or `eval $'\ngh pr merge 5'` | pass | pass | pass | refuse |
| `$'\tgh' pr merge 5`, which runs a program named with a tab before `gh`, not gh: bash says it is not found | pass | pass | refuse | pass |
| `echo $'\c\\'; gh pr merge 5 #'` and `echo $'\c'; gh pr merge 5 #'`, where bash ends the string before the merge and runs it. A version of this change passed the first, as below | refuse | refuse | refuse | refuse |
| `echo $'\c\'; gh pr merge 5 #'`, which bash prints as text | refuse | pass | pass | pass |
| sed's `1e` and `s\|...\|...\|e`, m4's `syscmd` from a here-string or a pipe, `tclsh <<< 'exec ...'`, `expect -c 'spawn ...'`, `vim -es -c '!...'`, `make -f -` from a here-string or a pipe, `sg users -c`, `tmux new -d`, `git rebase -x`, `git submodule foreach`, `echo '...' \| sudo bash`, `\| ssh host` and `echo gh pr merge 5 \| m4`, each given a merge | pass | pass | pass | refuse |
| A merge a variable holds, printed into such a program: `c='gh pr merge 5'` then `echo "$c" \| sudo bash`, `\| ssh host`, `sudo bash <<< "$c"`, `printf '%s' "$c" \| m4` or `tclsh <<< "$c"` | pass | pass | pass | refuse |
| `echo merge 5 \| xargs gh pr`, `printf merge \| xargs -I{} gh pr {} 5`, `echo '...' \| xargs -I{} bash -c {}` or `\| xargs bash -c` | pass | pass | pass | refuse |
| `N='5; gh pr merge 5'` then `bash -c "gh pr view $N"` or `eval "gh pr view $N"` | pass | pass | pass | refuse |
| curl's PUT to `.../pulls/5/%6derge`, or `gh api -X PUT repos/o/r/pulls/5/%6Derge` | pass | pass | pass | refuse |
| Merges held already, whose folder is in the table below: the review's two `if` forms with `echo then fi`, `while false; do echo do done; cd own; done`, `cd own x`, `declare "CD${z}PATH=../x"`, `export CD${z}PATH=...`, `printf -v CD${z}PATH ...` or `export CDPATH=../x` before `bash -c 'cd own && gh pr merge 5'`; and `echo case x in` or `echo if case x in esac` before a merge | refuse | refuse | refuse | refuse |
| `eval "$(cat <<'EOF' ...)"` running a merge, which the second draft held as one to grant and this change refuses unread | refuse | refuse | refuse | refuse |
| Cost: `gh pr comment 5 --body-file - <<< '...gh pr merge 5...'` | pass | pass | pass | refuse |
| The same from a heredoc | pass | pass | refuse | refuse |
| Cost: `sed -n '/gh pr merge/p' notes.md`, `rg 'gh pr merge' docs`, `git merge -m 'after gh pr merge 5' x`, a comment whose body spells a merge beside `\| jq .state`, and xargs's BSD `-J`, which GNU xargs does not have | pass | pass | pass | refuse |
| Cost of the third review: `echo if; case x in merge) gh pr view 5;; esac`, held by its pattern rule | pass | pass | pass | refuse |
| Fifteen ordinary commands, among them `gh pr view "$N" --json mergeable \| jq -r .mergeable`, `grep -rn 'gh pr merge' docs`, `git log --grep='gh pr merge'`, `git commit -m '...gh pr merge 5...'`, `gh pr comment 5 --body '...'`, `echo 'gh pr merge 5' >> notes.md`, `printf merge \| xargs -I{} echo {}`, `gh pr list ... \| xargs -n1 gh pr view --json mergeable`, `cd -- own && gh pr view 5 --json mergeable` and `if true; then echo then fi; fi; gh pr view 5 --json mergeable` | pass | pass | pass | pass |

A gh that only logs its folder stood first on PATH again, for sixteen
folder forms, and each also ran in bash 5.2.21 with that gh, to show where
bash runs it:
- After the review's `if true; then echo then fi; else cd other; fi` and
  `if false; then echo then fi; ! cd other; fi`, bash stays where it was.
  1.25.0 and `main` asked gh there; the second draft asked about `other`,
  the regression the review found. After
  `while false; do echo do done; cd other; done` bash stays too, and all
  three asked about `other`. This change leaves the folder untold for each.
- After `cd other x` bash stays where it was, as `cd` refuses two
  folders. 1.25.0, `main` and the second draft asked about `other`.
- After `cd -L -- other` and `cd -Pe other` bash moves to `other`. The
  earlier builds left the folder untold; this change asks about `other`.
- With `CDPATH` set by `declare "CD${z}PATH=..."`, `export CD${z}PATH=...`,
  `printf -v CD${z}PATH ...`, or by `export CDPATH=...` around `bash -c` or
  `eval` text, bash went to the `CDPATH` folder's `own`. Every earlier build
  asked about the first folder's `own`. This change leaves the folder
  untold.
- `export FOO=...` before `cd own`, and `bash -c 'cd own && ...'` with no
  `CDPATH`, are followed to the first folder's `own`, where bash goes.

The 159 commands, the 73 and the twenty-one folder forms above answer the
same on this change as on the second draft.

Bash 5.2.21 was run on the forms these rules rest on, with harmless
commands in place of a merge:
- a reserved word as an argument, as in `echo if case x in; echo AFTER`,
  is printed, and the command after it runs;
- `echo then fi` inside an `if` prints `then fi` and closes nothing;
- text left inside a `case` or an `if` is a syntax error, after the lines
  before it run;
- `cd own x` and `cd -x own` stay where they were; `cd --`, `-P` and `-LP`
  move;
- `$'\techo' hi` is not found and exits 127; `bash -c $'\techo TAB'` and
  `eval $'\necho NEWLINE'` run; `$'a\0b'` is `a`, `$'\cA'` is the byte 1 and
  `$'\q'` keeps its backslash;
- `echo $'\c\\'; echo AFTER #'` prints the byte 28 and runs `echo AFTER`,
  `$'a\c'` is `a\c`, and `echo $'\c\'; echo AFTER #'` prints the rest as
  text;
- `declare "CD${z}PATH=/tmp"`, `export CD${z}PATH=/tmp`,
  `printf -v CD${z}PATH /tmp` and `declare "$a"` with `a='CDPATH=/tmp'` each
  set `CDPATH`;
- xargs appends its input's words, `-I{}` puts its line in place of `{}`,
  and `xargs -I{} bash -c {}` runs its input as commands;
- `bash -c "echo view $N"` and `eval "echo view $N"` with `N='5; echo X'`
  run `echo X`.

Each program the review named, other than `sg` and `tmux`, was run with a
command that touches a file in place of a merge. sed's `1e` and `s|...|e`,
m4's `syscmd` from a here-string, a pipe or a variable printed into it,
`tclsh`'s `exec`, `expect`'s `spawn`, `vim -es -c '!...'`, `make -f -` from
a here-string or a pipe, `git rebase -x`, and xargs giving `bash -c` or
`touch` its words, each ran it. `git submodule foreach`, in a repository
with no submodule, ran nothing, as it runs its command once in each.
`tmux` would start a server of its own, and `sg` may ask for a password, so
neither was run; for them the rule rests on their manuals.

The grant mod in `main` and in the second draft does not send
`bash -c $'\tgh pr merge 5'`, `eval $'\ngh pr merge 5'`,
`make -f - <<< $'all:\n\tgh pr merge 5'`, curl's `%6derge` or gh's
`%6Derge` to the gate; this change's does.

A check of this change before it was committed found that its first
reading of `$'...'` read each escape as it looked for the string's end.
`\c` then took the character after it: in `$'\c\\'` one backslash, which
left the second to escape the closing quote, and in `$'\c'` the quote
itself. So `echo $'\c\\'; gh pr merge 5 #'` read as one `echo` of
quoted text and passed, where `main` and the second draft refuse it and
bash runs the merge. It now finds the end as bash does, and the test
reads both forms.

### The review of the third draft

Sixty-three more commands ran the same way on four builds: 1.25.0, `main`,
the third draft at `d27e7a74`, and this change.

| Command | 1.25.0 | main | Third draft | This change |
| --- | --- | --- | --- | --- |
| `time -- [[ x && case == in ]]; gh pr merge 5; case y in y) :;; esac`, the same after `time -p --` or `coproc n`, and after `time --` with gh api's `PUT`, curl's, a GraphQL merge or `--repo me/pub` in place of the merge | refuse | refuse | pass | refuse |
| `time -- gh pr merge 5`, `time -p -- gh pr merge 5`, `coproc n time -p case x in; gh pr merge 5`, and the review's controls: the first form without `time --`, or after `time` or `time -p` | refuse | refuse | refuse | refuse |
| `case x in gh\ pr\ merge\ 5) :;; esac`, or `case $t in gh\|curl) ...` or `merge) ...` beside `gh pr view 5 --json ...` | pass | pass | pass | refuse |
| `gh pr $'\x6d'\erge 5`, `mer$'\x67'\e`, `$'\x6d'e\rge`, gh api's `PUT` to `.../$'\x6d'\erge`, `$'\x63'u\rl -X PUT .../merge`, or `printf '\tgh pr merge 5' \| bash` | pass | pass | refuse | refuse |
| `gh pr mer$'\547'e 5`, `$'\555'erge`, `bash -c $'\cIgh pr merge 5'` or `eval $'\cjgh pr merge 5'` | pass | pass | refuse | refuse |
| After a `$'` that begins no string, as `grep -c 'x$' /dev/null;`, `echo '$';` or `echo "$'";`: `gh pr $'\x6d'\erge 5`, `mer$'\x67'\e`, `eval $'\cjgh pr merge 5'`, `bash -c $'\t'cu\rl\ -X\ PUT\ .../merge` or `$'\x63'u\rl -X PUT .../merge` | pass | pass | refuse | refuse |
| `x=$'\x6d'\erge; gh pr $x 5`, alone or after `grep -c 'x$' /dev/null;` | pass | pass | pass | refuse |
| `echo merge 5 \| busybox xargs gh pr`, or `toybox xargs` | pass | pass | pass | refuse |
| `busybox sh -c 'gh pr merge 5'` or `busybox timeout 5 gh pr merge 5` | pass | pass | refuse | refuse |
| `a=(<(gh pr merge 5))` | pass | pass | refuse | refuse |
| `gh pr mer$"g"e 5` or `c$"u"rl -X PUT .../merge`, held as text a locale may translate | pass | pass | refuse | refuse |
| `a=($(gh pr merge 5))`, or `if false; then a=(fi); fi; gh pr merge 5` | refuse | refuse | refuse | refuse |
| Costs kept: `sed -n '/gh pr merge/p' notes.md`, `rg 'gh pr merge' docs` and `N=5; bash -c "gh pr view $N --json mergeable"` | pass | pass | refuse | refuse |
| Sixteen ordinary commands, among them `time -- cargo test merge_gate`, `time -p -- gh pr view 5 --json mergeable`, `coproc n [[ -n x ]]; gh pr view 5 --json mergeable`, `case $(gh pr view 5 --json state -q .state) in MERGED) ...`, `busybox --list`, `echo 5 \| busybox xargs gh pr view --json mergeable` `a=(fi done esac); gh pr view 5 --json mergeable` and `a=() && gh pr view 5 --json mergeable` | pass | pass | pass | pass |

The escape forms are the grant mod's regression: the third draft's gate
refuses each when given it, but its module sends none of the first row's,
and neither `main`'s nor the third draft's sends the second row's. This
change's module sends each.

The forms after a `$'` that begins no string were found while checking
this change before it was sent. A draft of it read bash's escapes from the
first `$'` only, so such a `$'` before a merge's put each string after it
out of step: its module sent none of those forms, and its gate passed
`grep -c 'x$' /dev/null; x=$'\x6d'\erge; gh pr $x 5`, which it refused
alone. Each then read from each place a string may begin, which the
fourth review found still out of step, as its section below says. The
third draft's and `main`'s modules send none of those forms either.

A gh that only logs its folder stood first on PATH again, for seventeen
folder forms, and each also ran in bash 5.2.21 with that gh:
- After `time -- if false; then cd other; fi`, `time -p --` with the same,
  and `time -- while false; do cd other; done`, bash stays where it was.
  1.25.0's and `main`'s gates asked gh there; the third draft asked about
  `other`, the regression the review found. This change leaves the folder
  untold, as every build does after `time if false; ...`.
- After `a=(fi)`, `a=(done)`, `a=(})`, `a=(esac)`, `a+=(fi)`, a newline in
  an array and a comment in one, each inside an `if`, `while` or `case` not
  taken, bash stays where it was. `main` and the third draft asked about
  `other`; this change leaves the folder untold.
- After `=() { cd other; }`, `"a"=() { cd other; }` and
  `a$z=() { cd other; }`, bash defines a function, or refuses its name,
  and never runs the body. 1.25.0 and `main` asked gh in the first folder,
  where bash runs it; the third draft read `()` there as a subshell and
  left the folder untold. This change reads a function there, as bash
  does, and holds the command.
- `time cd other` and `a=(fi done); cd other` are followed to `other`,
  where bash goes.

Bash 5.2.21 was run on each form the review named and on the forms these
rules rest on, with a harmless command, or a gh and a curl that only print
their words, in place of a merge:
- `time --`, `time -p --` and `coproc n` before `[[ x && case == in ]]` run
  the command after it; after `time --` or `time -p`, a `-p` is run as a
  program and an `if` after it is its word;
- each array form closes nothing, a `#` inside a word is the word's, a
  newline is a space between words, `$(...)`, `<(...)` and backquotes run,
  and `a=(x;y)`, `a=( (x) )`, `a=(x|y)`, `a=(x&y)`, `a=(x>y)`,
  `"a"=(fi)` and `a$x=(fi)` are syntax errors;
- with a heredoc waiting, a newline in an array leaves its body to be read
  after the array, and bash read every later line as that body, its
  delimiter lost, so the merge after it did not run;
- `$'\547'` is `g`, `$'\555'` is `m`, `$'\cI'` a tab and `$'\cj'` a
  newline, `\e\r` outside quotes is `er`, and printf reads `\547` as `g`;
- a `$'` inside single or double quotes begins no string, and a `$'...'`
  after it is still one, so each form after such a `$'` and
  `x=$'\x6d'\erge; gh pr $x 5` reach gh or curl as a merge;
- `gh pr me${x}rg\e 5`, `curl ... %6Derg\e`,
  `printf '\tg${x}h pr merge 5' | bash` and
  `printf '.../%%6D\x65rge' | bash` reach gh or curl as a merge, each
  found by one spelling only;
- busybox 1.36.1 runs its applet for `xargs`, `sh -c`, `env` and
  `timeout`. toybox is not installed here, so for it the rule rests on its
  manual;
- `=() {...}` defines a function named `=`; `"a"=() {...}`, `a$z=() {...}`
  and `a${z}=() {...}` are read as functions whose name bash refuses when
  it runs, and the command after them runs; `a=() {` and `a[1]=() {` are
  syntax errors;
- `c$"u"rl` runs curl, and `mer$"g"e` and `$"g"h` spell a merge and gh.

The 159 commands, the 73, the 83, the second review's 12 held, 7
neighbouring and 4 control forms, and the 41 folder forms before answer
the same on this change as on the third draft, apart from
`case x in merge) gh pr view 5;; esac` and
`echo if; case x in merge) gh pr view 5;; esac`, which the pattern rule now
refuses. Each of them, and the 54 before them, answered the same again
once both readers read from each place a `$'...'` string may begin.

### The review of the fourth draft

Seventy-eight more commands ran the same way on three builds: `main`, the
fourth draft at `fbce7ade`, and this change. The first thirteen ran on the
second draft at `36be3362` too. Each also ran in bash 5.2.21 with a gh and
a curl that only print their words and the folder they ran in, apart from
the one under `sudo`.

| Command | main | Second draft | Fourth draft | This change |
| --- | --- | --- | --- | --- |
| `gh pr $'\x6d'$(: "$'")\erge 5`, `...e\rge 5`, the same with the `$'` in backquotes, before it too, or as `$(: '$')`, gh api's `PUT` to `.../$'\x6d'$(: "$'")\erge`, and `$'\x63'$(: "$'")u\rl -X PUT .../merge` | pass | refuse | refuse | refuse |
| `gh pr mer$'\x67'${z:+"$'"}\e 5` | pass | refuse | pass | refuse |
| `eval $'\cj'cu$(: "$'")\rl' -X PUT .../merge'` and `bash -c $'\t'cu$(: "$'")\rl$'\cI'-X\ PUT\ .../merge` | pass | pass | refuse | refuse |
| Controls: `gh pr $'\x6d'$(: "x")\erge 5`, the review's form without `--repo`, and `eval $'\cj'g${z:+"$'"}h' pr m'\erge' 5'` | pass, pass, refuse | refuse | refuse | refuse |

| Command | main | Fourth draft | This change |
| --- | --- | --- | --- |
| `g=gh m=merge; false && a[0; case x in ]=1; $g pr $m 5; case y in y) :;; esac`, with `a[ 0;` or `b[0; ... ]+=1` | pass | pass | refuse |
| `if false; then a[ 0 ]=(fi); cd own; fi; gh pr merge 5`, with `+=`, `a[1 + 1]`, `while ... (done)` or `a[0; fi ]=1`, and fourteen neighbours: after `x=1`, `x="1"`, `>f` or `2>f x=1`, at the start of an array's word, with a `[`, a quote, a backslash, a substitution, a parameter or `$'\x5d'` holding a `]` in the subscript, or with a newline. Held already; the folder each reads is in the unit tests | refuse | refuse | refuse |
| `x='b[$(gh pr merge 5)]'; a=([$x]=1)`, and with `echo done`: bash runs nothing | pass | pass | pass |
| `echo merge 5 \| ionice xargs gh pr`, and eleven like it behind `ionice -c 3`, `nice ionice`, `ionice ... env`, `taskset`, `chrt`, `flock`, `setsid` or `setsid -w`, or with a here-string; `echo merge 5 \| sudo xargs gh pr`, not run in bash | pass | pass | refuse |
| `m=merge; ionice -c 3 gh pr $m 5`, `m=merge; taskset 1 gh pr $m 5` and `printf merge \| ionice xargs -I{} gh pr {} 5` | pass | pass | refuse |
| `c='gh pr merge 5'; ionice -c 3 sh -c "$c"`, the same under `chrt -o 0 bash -c`, and `echo 'gh pr merge 5' \| ionice xargs -I{} sh -c {}` | pass | refuse | refuse |
| Three controls: `... \| ionice xargs -n1 gh pr view --json mergeable`, `taskset 1 xargs -I{} gh pr view {} --json mergeable` and `ionice -c 3 gh pr view 5 --json mergeable` | pass | pass | pass |
| `if false; then echo "${x:-"; fi; cd own; if :; then "}"; fi; gh pr merge 5`, with `'`, `${x:+`, `${x#` or `${x/`, unquoted, or as `$[1; ... 1]`. Held already; the folder each reads is in the unit tests | refuse | refuse | refuse |
| Control: `if false; then : ${x:=a b}; fi; cd own; gh pr merge 5`, where bash moves to `own` | refuse | refuse | refuse |
| curl's `PUT` to `.../pulls/5/%6''derge`, `%6\derge`, `'.../%6'"d"erge`, `m%6\5rge` or `%$'\x36'derge`, and gh api's to `.../%6''Derge` or `%6\Derge` | pass | refuse | refuse |
| Limit: curl's `PUT` to `.../pulls/5/%6${z}derge`, which no grant mod sends | pass | refuse | refuse |
| Controls: curl's `PUT` to `.../%6derge`, and `curl -s .../pulls/5/%6''d` | pass, pass | refuse, pass | refuse, pass |

The gate's verdict counts only for what the grant mod sends it. Given each
command, the grant mod's own module, as the test runs it, sends the gate:

| Command | main | Second draft | Fourth draft | This change |
| --- | --- | --- | --- | --- |
| The first row's seven, and `gh pr mer$'\x67'${z:+"$'"}\e 5` | no | yes | no | yes |
| The third row's two | no | no | no | yes |
| The first two controls | no | yes | yes | yes |
| The `eval` control | no | no | yes | yes |
| The split `%HH` forms but `%$'\x36'derge` | no | no | no | yes |
| `%$'\x36'derge`, and the control `.../%6derge` | no | no | yes | yes |
| The limit, and `curl -s .../pulls/5/%6''d` | no | no | no | no |

With no gh login, every build refuses each merge it reads, so for the
subscript and parameter forms the folder the gate reads shows only in the
unit tests. Bash runs each of those merges in the folder it started in,
since the `cd` is in the branch not taken. The unit tests hold that this
change's gate leaves the folder untold for them, and they fail on the
fourth draft's, the third draft's and `main`'s gate, as below.

Twelve ordinary commands ran through the gate alone, for this change's
costs, in Consequences: `x=1 a[ 0 ]=2`, `curl ... | sudo bash` and text
nested five deep pass on the fourth draft and refuse here. `a[0]=1`,
`a[i]=1` and `curl ... | bash` refuse on both, four deep passes on both,
and `sed -n $'/hugh merge/p' notes.md` passes on both. The other four
pass on every build.

The grant mod's own module took 21 ms on a command of the review's size,
1.9 MB with 32 `$'` strings, against 398 ms for the fourth draft's and 14
ms for the third draft's, each the median of five runs by node's clock
here. An earlier run measured 37, 725 and 27 ms. The review measured 6 to 7
s against 0.4 s on its own machine. Each spelling is one pass, so the time
grows only with the command's length. These figures are one machine's. The
fifth review measured the fifth draft's module at 728 ms against 5,303 ms
for the fourth draft's on its own 1.9 MB command, and found that 21 ms
needs a filler holding fewer escapes than its own.

The 159 commands, the 73, the 83, the third review's 54 and 9, the second
review's 12 held, 7 neighbouring and 4 control forms, and the 58 folder
forms before answer the same on this change as on the fourth draft. Of the
471 commands the probes hold, this change's module sends the gate each one
the fourth draft's sends, and ten more, each one of this review's forms.

Bash 5.2.21 was run on the forms the new rules rest on, with harmless
commands in place of a merge:
- `name[` is read to its matching `]` at a command's start, after `if`,
  `then`, `do`, `!`, `time`, `time -p`, `coproc`, `{`, `(`, `|`, `&&`,
  `||` and `;`, after an assignment, its value plain, quoted or computed,
  and after redirections that begin a command, as `>f a[...` and
  `>f x=1 a[...`. It is split as an argument, after a redirection that
  follows an assignment, as `x=1 >f a[...`, in a `case` word, after `for`,
  inside `[[`, and where the name or an assignment's name before it is
  quoted or computed, as `"a"[ 0 ]=1` or `x$y=1 a[...`. In an array it is
  read whole only at a word's start;
- inside one, a `[` nests, a backslash takes the next character, and a
  quote, a backquote, `$(...)`, `${...}`, `$[...]` and `$'...'` are read
  whole, so `a[b[ 0 ] ]=1` and `a[ $(echo ] >/dev/null; echo 0) ]=1` assign
  `a[0]`, and `a[' ]' ]=1` and `a[\ \] ]=1` fail on the whole subscript;
- `a[0; echo X ]x=1` is a command bash does not find, and a `b[0; ...` after
  it is split, so its `echo` runs;
- an unquoted `${...}` or `$[...]` is read whole, spaces and `;` among it,
  and inside double quotes a `"`, a backquote, `$(...)` and `${...}` nest
  in one; a `'` there nests only in a pattern or a change of case (see the
  fifth review below);
- each runner above runs xargs's command with the words xargs gives it, and
  `sh -c` or `bash -c` with the string;
- each split `%HH` form reaches curl or gh with the escape whole, as
  `%6derge`, or `m%65rge` from `m%6\5rge`. Whether GitHub's API then reads it as `merge` was not tried.

### The review of the fifth draft

The fifth review found one regression, in the fifth draft's reading of a
quote inside a double-quoted `${...}`, with two sides:
- a `'` in the word of a default, an alternative, an assigned value or an
  error message hid a substitution bash runs, as
  `echo "${x:-'$(gh pr merge 5)'}"`;
- in posix mode and in dash, the first `}` there ends the `${...}`, so
  `set -o posix`, then `echo "${x:-'}"; gh pr merge 5; echo "'}"` on the
  next line, runs the merge, as do `POSIXLY_CORRECT=1`, `bash --posix`,
  `sh -c`, `dash -c` and `sh <<'E'` with that line.

Thirty-six commands ran in bash 5.2.21, with a gh and a curl that only
print their words, and through `provider-merge` on `main`, the fifth draft
at `33ca4577`, and this change:
- 26 that run a merge, or would in posix mode or dash: the fifth draft
  passes 24 of them and this change refuses all 26, as `main` does. Among
  them are the `-`, `+`, `=` and `?` forms with a `:` and without, a
  backquote, a nested `${...}`, a `$'...'`, a heredoc, an assignment, a
  command after a successful `gh pr view`, curl's `PUT` to `.../merge`, and
  the posix and dash forms above;
- 8 controls, where bash runs nothing quoted (a pattern, a change of case,
  a pattern around a default, and a default outside double quotes): this
  change passes each, as the fifth draft did; `main` refuses six;
- two of the costs in Consequences, the `:?` form and the line that runs
  a merge only in posix mode or dash: this change refuses them, as `main`
  does. The third, `gh pr view 5 --json mergeable --jq "${Q:-'.mergeable'}"`,
  ran on its own: `main` and the fifth draft pass it, this change refuses
  it, and all three pass it written without the `'`.
Bash 5.2.21, `bash --posix` and dash were also run with each operator,
showing that a pattern or a change of case runs nothing inside its quotes
and that the others run what is between two `'`.

The fifth review also found four earlier pieces that failed no named test.
Each now has an assertion on the words the gate reads, checked against bash
5.2.21, and none was an equivalent mutant: the rule that a `case` command's
patterns read no subscript whole (`case x in y) :;; a[ ) echo two;; esac`),
the same inside `[[ ... ]]` (`[[ -n x && a[ ]] && echo t`), after a word
bash computes a part of (`$(true)a[ 0 ]=1`), and a backslash before a
newline in a `${...}` (`${x:-a\` and `b` on the next line, `ab`). Its four
older gaps are listed in "What this does not cover".

### Tests and ablations

Forty-six tests are new, and each passes on this change: forty-five for
the gate and the grant mod's. The seventeen for the gate from the first
draft fail on `main`'s gate. The ten from the first review fail on
`main`'s gate and on the first draft's. The ten from the second review fail
on the second draft's gate. Nine of them fail on `main`'s too, each on an
assertion, once seven helpers they use are added to `main`'s tests. The
tenth, `a_reserved_word_given_as_an_argument_opens_nothing`, passes on
`main`, which never had the regression it tests. The grant mod's fails with
`main`'s module and with the first draft's. The five forms it gained in the
second review are sent by neither `main`'s module nor the second draft's.
The four from the third review fail on the third draft's gate and on
`main`'s, and so do the `$'...'` test's assertions from that review on the
third draft's. The grant mod's fails with the third draft's module, first
on `gh pr $'\x6d'\erge 5`, and with `main`'s.

The four from the fourth review, for subscripts, parameters, what a runner
leaves unread, and a bracket, quote or substitution in a subscript, fail on
the fourth draft's gate, the third draft's and `main`'s, each on an
assertion once the test helpers they use are added to those tests (on its
own the test does not compile there), first on `if false; then a[ 0 ]=(fi); cd own; fi; gh pr merge 5`
or the like. The `$'...'` test's new assertions fail on the fourth draft's,
first on `gh pr $'\x6d'$(: "$'")\erge 5 --repo 'o/r'`; the `%HH` test's
fail on the fourth and third drafts', first on `.../pulls/5/%6''derge`. The
grant mod's fails with the fourth and third drafts' modules, first on that
address, and with `main`'s, first on `gh pr mer''ge 5`.

On this change's code, 335 pieces of the change were removed one at a time,
and a named test failed each time. The last two ran once the busybox test
had gained the assertion below; the rest ran on the source before it, which
differs only in that test:
- the earlier drafts' 188 but one, retired below, eight of them moved to
  where the fourth review left their code;
- the second review's 57, three of them moved;
- the third review's 68 but 28, whose code the fourth review deleted, one
  of them moved. Among them are the two that the third review found failed
  no test: a named coprocess reading every word as a command's start, and
  an octal escape kept past its low byte;
- 51 for the fourth review's pieces and the two found while checking it.

The grant mod's are among them, since its test runs the module. The source
was restored byte for byte after each. A first full run on the third
draft's source found four earlier pieces that failed no named test: three
of the gate's spellings and the grant mod's `$` before a quote. The forms
only those spellings find had moved into the `$'...'` test, and `$'...'` is
now read whole, so `mer$'g'e` no longer needs its `$` taken out. Each now
names the `$'...'` test, and the grant mod's test gained `mer$"g"e` and
`c$"u"rl`.

Runs on this source found eleven pieces that failed no named test:
- seven readings inside a subscript, whose forms the tests held only as
  refused, which a subscript read wrongly is too. They now have assertions
  on the words the gate reads, each checked against bash 5.2.21;
- busybox and toybox running their applet, whose xargs forms are now
  refused as another program's too. Their test now holds a wrapper
  applet's merge read as the merge itself;
- the gate's and the grant mod's every-escape reading with `%HH` read,
  deleted as below.

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
    which the `case` already makes uncertain;
- after the second review, the spelling with its escapes read and not its
  `%HH` escapes, in the gate and the grant mod. The spelling that reads
  both covers it, since reading `%HH` only adds text. Removing it failed no
  test, and an earlier ablation, which stops the first drafts' spellings
  taking quotes out, failed none while the spelling that reads both was
  left to do it;
- after the third review, the guard that kept a word ending in `=` before
  `()` from naming a function. An array's own reading takes `a=()` first,
  and bash reads any other such word as a function's name, as `=()` and
  `"a"=()`, which the guard had the gate read as a subshell. Its ablation is
  the one retired, and putting it back now fails a test;
- after the fourth review:
  - the readings from each place a `$'...'` string may begin, their bound
    of 32, and the grant mod's reading of `\cX`. The numbered reading and
    the rule for gh or curl beside a `$'` replace them;
  - the every-escape spelling with its `%HH` escapes read, in the gate and
    the grant mod. The numbered reading makes each letter that one makes,
    so removing it failed no test. Looking at why showed that both read
    `%HH` escapes before taking quotes out, which is how the split forms in
    the Context passed.

After the fifth review one test is new,
`a_quote_in_a_word_bash_expands_in_double_quotes_is_not_read_past`. It
fails on the fifth draft's gate, first on
`echo "${x:-'$(gh pr merge 5)'}"`, and passes on this change. Of the 17
pieces its fix adds, 16 failed it when removed one at a time; the
seventeenth, putting back the state of the word around a nested `${...}`,
failed none, and the test gained a form for each way that can go wrong.
That piece and the four earlier pieces the fifth review found were then
removed one at a time, and each failed a named test. The source was
restored byte for byte after each. The earlier drafts' 335 ablations, the
transplants onto older refs and the earlier probe sets were not run again
for this change, whose code differs from the fifth draft's only in the
lexer's reading of `${...}`.

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
- The fixes of the second review refuse more ordinary commands once they
  mention a merge:
  - a program the gate does not know, given a word that spells a merge and
    gh or curl, as `sed -n '/gh pr merge/p' notes.md` or
    `rg 'gh pr merge' docs`, and git's other commands given one, as
    `git merge -m '...gh pr merge 5...'`. grep, `git log --grep` and
    `git commit -m` pass;
  - such a program fed by a pipe or a redirection in a command where a word
    spells one, as a `gh pr comment --body '...gh pr merge 5...'` beside
    `... | jq`;
  - a here-string whose text reads as a merge command, as
    `gh pr comment 5 --body-file - <<< 'Run gh pr merge 5 ...'`, which
    `main` and the second draft pass. The same in a heredoc, which `main`
    passes, the second draft refuses already;
  - shell text or `eval` words that bash computes in part, as
    `bash -c "gh pr view $N"`, and `eval "$(cat <<'EOF' ...)"`, whose
    merge the second draft held to grant;
  - xargs with an option the gate does not know, or giving gh or a shell
    its words, as `... | xargs gh pr merge`;
  - a variable whose name bash computes, as `declare "$n=1"` or
    `read -r "$v"`, holds every merge in the command, and a relative `cd`
    after it leaves the folder untold.
- `cd own x` and `cd -x own`, which bash refuses, now leave the folder
  untold, so a merge after them must name its repository.
- The fixes of the third review refuse one more kind of ordinary command
  once it mentions a merge: a `case` pattern that names gh, curl or merge
  as a word of its own, as `case x in merge) gh pr view 5;; esac` or
  `case $t in gh) ...;; esac; gh pr view 5 --json mergeable`, which `main`
  and the third draft pass. `MERGED)` and `merged)` pass.
- The fixes of the fourth review refuse more ordinary commands once they
  mention a merge:
  - an assignment to an array element that the fourth draft split, as
    `x=1 a[ 0 ]=2; gh pr view 5 --json mergeable`. It is arithmetic, as
    `a[0]=1` there already was;
  - a shell run by `sudo` reading input the gate cannot see, as
    `curl -s https://example.test/i.sh | sudo bash`, which without `sudo`
    is refused already;
  - text nested more than four deep in `${...}`, `$[...]` or subscripts.
- The fix of the fifth review refuses a command that mentions a merge and
  holds a `'` or a `$'` in the word of a double-quoted default,
  alternative, assigned value or error message, as
  `gh pr view 5 --json mergeable --jq "${Q:-'.mergeable'}"`, which `main`
  and the fifth draft pass; written without the `'`, it passes. The same
  holds an owner's own merge, as
  `gh pr merge 5 --squash --body "${BODY:-'none'}"`: the gate refuses it as
  a command it cannot read, before it asks who opened the pull request.
  Written without the `'`, or with `"none"` in place of `'none'`, it is
  checked as any merge is. It also
  refuses `x=a; echo "${x:?'$(gh pr merge 5)'}"`, where bash runs nothing
  because `x` is set: the gate does not follow what a variable holds, and
  with `x` unset bash runs the substitution to print its error. And it
  refuses `echo "${x:-'}"; gh pr merge 5; echo "'}"`, which runs no merge in
  default bash and runs one in posix mode and dash. `main` refuses these
  two; the fifth draft passed them.
- In a command that holds a `$'` and mentions a merge, the grant mod sends
  the gate any command with "gh" or "curl" in it, even inside a word, as
  `sed -n $'/hugh merge/p' notes.md`. The gate reads its words as bash
  does and passes this one.
- The change is in `swarm-terminal-host` and the grant mod it writes. It
  reaches a worker only after the worker engine update, which stops running
  sessions.
