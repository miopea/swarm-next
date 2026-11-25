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
    is `--editor` or `--web`, any group of one-letter flags holding an `e`
    or a `w` among its flags, and `gh gist edit`, which opens the editor
    with no flag. Whether a letter of a group takes the rest as its value
    often depends on the command: `-f` is `--fill` to `gh pr create` and
    takes a value in `gh api`. So a group's flags end only where a value
    must begin: at a letter that takes a value in every gh command that has
    it, or at an `=`, after which gh's flag parser gives the letter before
    it the rest of the word. Those letters are
    `A B D F H L O R S T X b g j k o q`, read from `gh help reference` for
    gh 2.103.3. A command without one refuses it and runs nothing, and `-h`
    is not one, since it is `--help`. A later gh that gives one of them a
    flag that takes no value needs the list changed. So `-fw`, `-pw` and
    `-wq.title` count, and `-Ro/web`, `-Fnotes.md`, `-q.mergeable` and
    `-f=web` do not. A text flag's value does not count, because gh takes
    the next word as that value whatever it starts with.
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
  - A heredoc is standard input only with no descriptor before `<<`, or 0.
    One on another descriptor, as in `3<<'C'`, is given to the command but
    is not what it reads on standard input. Its body is still read as
    commands.
  - A descriptor is what bash reads as one: digits with no quote or
    backslash in them, or a `{name}`, for which bash opens a new descriptor,
    as in `{fd}<<'C'`. A quoted or escaped digit, as in `"3"<<'C'` or
    `\3<<'C'`, is a word of the command, and its heredoc is standard input.
  - bash also sets an array element to a new descriptor, as in
    `{a[1]}<<'C'`. Which subscripts it takes is its own parser's to say:
    bash 5.2.21 takes `{a[1]}`, `{a[x+1]}` and `{a["1"]}`, and reads
    `{a[1]]}`, `{a[]}` and `{a[1]x}` as words. So a command with a braced
    word holding a `[` right before a redirection is not read, and is
    refused when it mentions a merge. Any other braced word, as `{1x}` or
    `{}`, is a word of the command, as bash reads it, and its heredoc is
    standard input.
  - Anything put onto descriptor 0 by name is standard input the gate does
    not follow: a dup, as `0>&3` putting back the pipe that `3>&0` saved
    while a heredoc took its place, or a file, as `0>f`. A dup onto another
    descriptor, as `2>&1`, leaves standard input as it was.
  - A GraphQL merge mutation in that request is a merge.
  - For the merge API, a `sha` in it names the head as hidden, as
    `curl -d` already did.
- **A GraphQL request the gate cannot see is held as a merge it cannot
  read.** It is refused as 1.25.0 refused it, with the note to give the
  request in the command itself. gh reads the endpoint `graphql`, `/graphql`
  and a URL whose path ends in `/graphql`, such as
  `https://api.github.com/graphql`, as the same one, and so does the gate.
  It reads them in any case of their letters, as in `gh api GraphQL`,
  because whether GitHub does was not run.
  It reads a group of one-letter flags as gh's flag parser does: the
  letters before the first that takes a value take none, and that one takes
  the rest of the word, less one `=`, or the next word. So `-iF query=@q`
  is `-i` and then `-F`. A letter that takes no value ends the group at an
  `=` and takes the rest as its own: `-i=t -X PUT` is `--include` set to
  true and then `-X PUT`, and all of `tX` in `-i=tX` is the value of `-i`.
  gh 2.103.3 was run on `-i=t`, `-i=f` and `-i=tX`. That is:
  - `gh api graphql` reading its request from a file: a field's `key=@file`
    after `-F` or `--field`, or `--input file`. `/dev/fd/0` counts as a
    file. `-f` sends an `@` as it is.
  - `gh api graphql` sending standard input the gate cannot see: a file, a
    here-string or a descriptor after `<`, a pipe from anything but `cat`
    with no file or `tee`, or nothing at all. A heredoc is seen when it is
    the command's own on standard input, or passed down the pipe by `cat`
    or `tee`.
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
  - curl's data attached to its flag, as in `curl .../graphql -d'{...}'`;
  - the branch merges API, `gh api -X POST repos/o/r/merges`, which merges
    a branch, such as a pull request's head, into another;
  - an unreadable command that mentions `enqueuePullRequest` but not
    "merge". The hook sends it to the gate, which looks only for "merge";
  - curl's own config, `~/.curlrc`, which curl reads unless told not to,
    written by the same command with the data or the URL, then a bare
    `curl`.

  That list also named a GraphQL merge sent to `/graphql` with an inline
  `-f query='mutation { mergePullRequest(...) }'`. The gate now reads
  `/graphql` as GraphQL, so that one is refused.
- **Found by the third review, and passing on this change and on 1.25.0.**
  To be fixed separately:
  - curl's config named with its value attached, `curl -Kq.cfg`;
  - curl's `--data-ascii @q.json`, another name for `-d`;
  - a URL curl expands, as in `'https://api.github.com/{graphql}'`, which
    curl's globbing turns into `/graphql`.

  The same review named `gh api -iX PUT repos/o/r/pulls/5/merge`, a REST
  merge whose `-X` sat in a group. The group reading above reads it as
  `-X PUT`, so it is refused, and so is curl given a decoy heredoc on
  another descriptor.
- **Found verifying the sixth version, and passing on this change and on
  1.25.0.** A readable GraphQL merge given to gh inside a `{ ...; }` group,
  piped in, as in `cat <<'A' | { gh api graphql --input -; }`, or as the
  group's own heredoc, as in `{ gh api graphql --input -; } <<'A'`. The gate
  does not read gh as the group's first command. An unreadable one is
  refused, as by 1.25.0, and so is the same in a `( ... )` subshell. To be
  fixed separately.
- **gh's other ways to run a program its config names.** Its pager runs only
  on a terminal, and an interactive prompt that opens the editor needs one
  too. A Claude worker's command has none: none of its standard input,
  output or error is a terminal. Neither was run. Nor was whether gh opens
  its editor for `-e`, `-fe` or `gist edit`, or its browser for `-w` or
  `-fw`, without a terminal: the gate holds them as if it does.
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
| A merge mutation written from an unreadable body to a file, then sent by `gh api /graphql -F query=@q.graphql`, or by `gh api graphql` with `-iF query=@q.graphql`, `-iFquery=@q.graphql`, `-F=query=@q.graphql` or `-iF=query=@q.graphql`; an unreadable heredoc holding one, piped by `cat` to `gh api graphql -F query=@-` beside a `3<<'C'` decoy; a gh config written from an unreadable body naming a browser or editor, then `gh pr create -fw`, `gh pr create -fe` or `gh gist create -pw f.txt`; a file written from an unreadable body that mentions a merge, then `gh pr comment 5 -Ro/web --body-file notes.md` | refuse | refuse |
| A merge mutation in a file written from a body readable as shell or by `printf`, then sent by `gh api /graphql --input q.json`, `gh api https://api.github.com/graphql --input q.json`, `gh api graphql -iF query=@q.graphql`, or `cat q.json \|` to `gh api graphql --input -` or `curl -d @- https://api.github.com/graphql` beside a `3<<'C'` decoy; a readable heredoc holding one, piped by `cat` to `gh api graphql --input -` beside the same decoy; `gh api /graphql -f query='mutation { mergePullRequest(...) }'`; `gh api -iX PUT repos/o/r/pulls/5/merge` and `gh api repos/o/r/pulls/5/merge -iX PUT` | pass | refuse |
| A file written from an unreadable body that mentions a merge, then `gh pr comment 5 -R o/web --body-file notes.md` or `gh pr comment 5 --body "- fixed the web view"` | refuse | pass |
| An unreadable heredoc holding a merge mutation, given as standard input to `gh api graphql -F query=@-` after `--jq "3"` or `--jq \3`, beside an innocent one piped by `cat`; an unreadable one piped by `cat` to `gh api graphql -F query=@-` beside a `{fd}<<'C'` decoy; a merge mutation written from an unreadable body to a file, then sent by `gh api GraphQL -F query=@q.graphql` | refuse | refuse |
| A readable heredoc holding a merge mutation, given as standard input to `gh api graphql --input -` after `--jq "3"`, or to `curl -d @- https://api.github.com/graphql "3"`, beside an innocent one piped by `cat`; one piped by `cat` to `gh api graphql --input -`, or `cat q.json \|` to it or to `curl -d @-`, beside a `{fd}<<'C'` decoy | pass | refuse |
| `gh api -i=t -X PUT repos/o/r/pulls/5/merge`, or with `-i=f` or `-i=F`; `gh api -i=t graphql` with an inline merge mutation, or with `-F query=@q.graphql` after a file written from an unreadable body | refuse | refuse |
| An unreadable heredoc holding a merge mutation, piped by `cat` to `gh api graphql -F query=@-` beside `3>&0 <<'C' 0>&3` or beside an `{a[1]}<<'C'` decoy | refuse | refuse |
| A heredoc readable as shell holding a merge mutation, piped by `cat` to `gh api graphql --input -` or `curl -d @-` beside `3>&0 <<'C' 0>&3`, or to `gh api graphql --input -` beside an `{a[1]}<<'C'` or `{a["1"]}<<'C'` decoy; a merge mutation in a heredoc after `{1x}<<'C'`, beside an innocent one piped by `cat`; `gh {a[1]}<<'C' pr merge 5` | pass | refuse |
| A file written from an unreadable body that mentions a merge, then `gh pr comment 5` with `-Ro/web --body-file notes.md`, `-Fnotes.md`, `-Rwebteam/site -F notes.md` or `-f=web --body-file notes.md`, or `gh pr view 5 --json mergeable -q.mergeable` | refuse | pass |
| A gh config written from an unreadable body naming a browser, then `gh pr view 5 -wq.title`, `gh pr create -fw` or `gh pr view 5 -c=t -w` | refuse | refuse |
| `jq -n` printing an innocent query into `gh api graphql --input -`, in a command that mentions a merge | pass | refuse |
| An innocent heredoc given to `gh api graphql --input -` with `2>&1` or `>&2`, or piped by `cat` beside a `{fd}<<'C'` decoy, in a command that mentions a merge | pass | pass |
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
only because it misread the body as shell.

The fifth version closed both and passed its own verification. Its third
review found four routes that 1.25.0 refuses when the body cannot be read,
and that it passed:
- `gh api /graphql` or the full URL, because the gate matched only the word
  `graphql`;
- `-F` in a group after a flag that takes no value, as `-iF`, because the
  gate split a group only when its first letter took a value;
- a decoy heredoc on another descriptor, which the gate took for standard
  input;
- `-w` or `-e` after `-f` or `-p`, letters that take a value in another gh
  command, where the gate stopped reading the group.

The sixth version closed those four. The same changes refuse the readable
and `printf` forms, which 1.25.0 passes. Fixing them found
`-F=query=@q.graphql` passing too: gh's flag parser takes the `=` off, and
the gate kept it. Verifying the sixth version found two more routes that
1.25.0 refuses when the body cannot be read, and that it passed:
- a quoted or escaped digit before `<<`, as in `--jq "3"<<'C'`, which it
  took for a descriptor, so it saw the decoy piped in as standard input and
  not the heredoc bash gives it. The descriptor rule brought this one in;
  the fifth version refused it;
- a `{name}` descriptor, as in `{fd}<<'C'`, which it took for standard
  input.

Bash 5.2.21 was run on both. The seventh version closed them and read the
endpoint in any case of its letters. Its review found three routes that
1.25.0 refuses, two of them only when the body cannot be read, and that it
passed:
- a letter that takes no value given one with `=`, as `-i=t` in
  `gh api -i=t -X PUT repos/o/r/pulls/5/merge`. gh's flag parser ends the
  group at the `=`. The gate read on to the `t` and took it for `-t`,
  whose value was then `-X`;
- `0>&3` putting back the pipe that `3>&0` saved after a heredoc, which
  the gate took for the heredoc on standard input;
- an array element as the descriptor, as in `{a[1]}<<'C'`, which the gate
  took for a word, with the heredoc on standard input. 1.25.0 also passes
  `gh {a[1]}<<'C' pr merge 5`, which bash runs as `gh pr merge 5`.

The same review named a cost: an `e` or a `w` in a value attached to its
flag, as in `-Fnotes.md` or `-q.mergeable`, refused a command that merges
nothing when it mentioned a merge and held an unreadable heredoc. It also
suggested reading any braced word before a redirection as a descriptor.
That is not done: bash reads `{1x}<<'C'` and `{}<<'C'` as a word with the
heredoc on standard input, so a merge in that heredoc would pass beside a
decoy piped in.

This decision is the eighth version, with those three closed and that cost
removed, and it has not been reviewed again. Both columns of the rows from
"A merge mutation written from an unreadable body to a file by `cat` or
`tee`" on were run on the built gates, the installed 1.25.0 and this
change, with gh out of reach.

A hundred and nineteen pieces were removed one at a time, and a named test
failed each time. Twenty-three more were removed for the second review's two
classes, with the same result. For the third review's four and the two its
verification found, forty-two were: twenty-two of those twenty-three again,
since the code behind the last is gone, and twenty new. Each made a named
test fail. For the fourth review's three and its cost, sixteen were, and
each made a named test fail.

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
  a merge. So is one beside a group of gh's one-letter flags with an `e` or
  a `w` among its flags, as in `-fw` or `-wq.title`. An `e` or a `w` in a
  value passes: given as its own word, as in `-R o/web`; attached after a
  letter that always takes one, as in `-Ro/web` or `-q.mergeable`; after an
  `=`, as in `-f=web`; or as a text flag's value.
- A heredoc on a descriptor other than 0, given by number or as a
  `{name}`, is not standard input. A gh or curl request read from standard
  input beside one is held unless what standard input really carries is
  seen. A quoted or escaped digit before `<<` is now a word of the command,
  as bash reads it, where 1.25.0 dropped it as a descriptor. A command with
  a braced word holding a `[` before a redirection, as `{a[1]}<<'C'`, is not
  read, and is refused when it mentions a merge. A request read from
  standard input after `0>&3`, or anything else put onto descriptor 0, is
  held.
- What another command prints into `gh api graphql --input -`, such as
  `jq -n '{query: ...}'`, is standard input the gate cannot see. So the
  request is held in a command that mentions a merge, even when it merges
  nothing. 1.25.0 passes it, and passes it with a merge mutation too.
- A GraphQL request to `gh api GraphQL` or another spelling of the endpoint
  in other letters is held as one to `graphql`, even if GitHub does not
  serve it.
- The change is in `swarm-terminal-host`. It reaches a worker only after the
  worker engine update, which stops running sessions.
