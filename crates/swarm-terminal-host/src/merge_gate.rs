//! Refuses an agent's merge of someone else's pull request in the operator's
//! own public repository, unless the operator granted that exact command
//! (ADR 0179).
//!
//! `swarm-terminal-host provider-merge <worker MCP config>` is run by the grant
//! mod's `tool.check` hook for a shell command that may merge a pull request,
//! with `{"command": "...", "cwd": "..."}` on standard input. The first line it
//! prints is the verdict:
//!
//! - `pass`: the command merges nothing the rule covers, so the worker's own
//!   permission checks decide exactly as they did before;
//! - `granted`: the operator granted exactly this command and Swarm spent that
//!   grant (ADR 0129). A grant Queen pressed (ADR 0158) does not count;
//! - `refuse`: anything else. The lines after it are what the worker reads.
//!
//! The rule covers a merge only in a PUBLIC repository whose owner is the
//! account the merge would be made as, of a pull request that account did not
//! open. A bot never is that account. The operator chose that scope: their
//! private repositories are already shared with people they chose, and every
//! other owner's repository keeps Queen's go-ahead.
//!
//! ⚠️ EVERY DOUBT IS A REFUSAL. A merge this cannot show to be outside the rule
//! is treated as inside it: a pull request gh cannot read, a folder it cannot
//! tell, a command it cannot parse. A refusal costs the worker one request to
//! the operator; a wrong pass merges a stranger's code into a repository that
//! every Hive installs from.
//!
//! ⚠️ IT READS COMMAND TEXT, SO IT HAS LIMITS. It sees `gh pr merge`, the merge
//! API through `gh api` or `curl`, GraphQL's merge mutations, and those inside
//! `bash -c`, `eval`, a substitution, or a heredoc. A heredoc that cannot be
//! read as commands, given only to programs that never run it, is data: a
//! pull request's body that says "merge" is not a merge (ADR 0188). A script
//! file, an alias, a here-string, or a query read from a file is not seen, and
//! neither is a session without Swarm's grant mod. ADR 0179 names what that
//! leaves uncovered.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde_json::Value;
use tokio::io::AsyncReadExt;

use crate::provider_grant;

/// One gh call. Three are made per merge, and the mod waits on all of them.
const GH_TIMEOUT: Duration = Duration::from_secs(6);
/// gh's answer for one pull request or repository is a few hundred bytes.
const MAX_GH_OUTPUT_BYTES: u64 = 256 * 1024;
/// `bash -c "eval '...'"` and deeper are read this far, then refused.
const MAX_NESTING: usize = 4;

/// What the mod is told.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Pass,
    Granted,
    Refuse(String),
}

impl Verdict {
    fn printed(&self) -> String {
        match self {
            Self::Pass => "pass".to_owned(),
            Self::Granted => "granted".to_owned(),
            Self::Refuse(why) => format!("refuse\n{why}"),
        }
    }
}

pub(crate) async fn run(config: Option<&str>) {
    let verdict = match config {
        Some(config) => match provider_grant::read_input().await {
            Ok(input) => gate(Path::new(config), &input, &GhCli::default()).await,
            Err(error) => Verdict::Refuse(could_not_check(&error)),
        },
        None => Verdict::Refuse(could_not_check(
            "usage: swarm-terminal-host provider-merge <worker MCP config>",
        )),
    };
    println!("{}", verdict.printed());
}

/// Decides one command, spending the operator's grant only when the rule
/// covers it and nothing else refuses it first.
async fn gate(config: &Path, input: &[u8], gh: &GhCli) -> Verdict {
    let (command, cwd, _) = match provider_grant::command_input(input) {
        Ok(read) => read,
        Err(error) => return Verdict::Refuse(could_not_check(&error)),
    };
    let merges = match merges_in(&command, cwd.as_deref().map(Path::new), 0) {
        Parsed::Merges(merges) => merges,
        Parsed::Unclear => {
            return judge_or_grant(
                config,
                input,
                vec![Judged::Unread {
                    what: "the command mentions a merge but cannot be read".into(),
                    head: Head::Unnamed,
                }],
            )
            .await;
        }
    };
    let mut judged = Vec::with_capacity(merges.len());
    for merge in merges {
        judged.push(match facts(gh, &merge).await {
            Ok(facts) => Judged::Read {
                head: merge.head,
                facts,
            },
            Err(what) => Judged::Unread {
                what,
                head: merge.head,
            },
        });
    }
    judge_or_grant(config, input, judged).await
}

async fn judge_or_grant(config: &Path, input: &[u8], judged: Vec<Judged>) -> Verdict {
    match judge(&judged) {
        Judgement::Pass => Verdict::Pass,
        Judgement::Refuse(why) => Verdict::Refuse(why),
        Judgement::NeedsGrant(why) => match provider_grant::ask_operator(config, input).await {
            Ok(true) => Verdict::Granted,
            Ok(false) => Verdict::Refuse(needs_grant(&why)),
            Err(error) => Verdict::Refuse(format!(
                "{} Swarm could not be asked whether the operator granted it: {error}.",
                needs_grant(&why)
            )),
        },
    }
}

/// The commit a merge command says it merges.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Head {
    /// `--match-head-commit <sha>`, or the merge API's `sha` field.
    Named(String),
    /// Named inside text this does not take apart, such as a GraphQL query's
    /// `expectedHeadOid`: the merge still cannot move past it, but this cannot
    /// say which commit it is.
    Hidden,
    Unnamed,
}

/// One merge a command makes, as far as its text says.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Merge {
    /// The folder gh resolves the repository in; `None` when the command moved
    /// somewhere this cannot follow.
    folder: Option<PathBuf>,
    /// `owner/name`, when the command names it.
    repo: Option<String>,
    /// The number, URL or branch it names; `None` means the folder's branch.
    pull_request: Option<String>,
    head: Head,
    /// Why gh cannot look the pull request up, when it cannot: a GraphQL node
    /// id, or a number that only arrives on standard input.
    unreadable: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Merges(Vec<Merge>),
    /// It mentions a merge and does not parse: quotes or a heredoc left open,
    /// nesting deeper than `MAX_NESTING`, or a shell that reads its commands
    /// from a pipe.
    Unclear,
}

/// What GitHub says about one merge.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Facts {
    /// The account gh is signed in as, which the merge would be made as.
    merger: String,
    repo: String,
    repo_owner: String,
    public: bool,
    number: u64,
    author: String,
    author_is_bot: bool,
    head: String,
}

impl Facts {
    /// The operator's rule reaches this merge (ADR 0179).
    fn covered(&self) -> bool {
        self.public
            && self.repo_owner.eq_ignore_ascii_case(&self.merger)
            && (self.author_is_bot || !self.author.eq_ignore_ascii_case(&self.merger))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Judged {
    Read { head: Head, facts: Facts },
    Unread { what: String, head: Head },
}

#[derive(Debug, PartialEq, Eq)]
enum Judgement {
    Pass,
    NeedsGrant(String),
    Refuse(String),
}

/// Whether every merge is outside the rule, or which ones need the operator.
fn judge(judged: &[Judged]) -> Judgement {
    let mut needing = Vec::new();
    for merge in judged {
        match merge {
            Judged::Read { facts, .. } if !facts.covered() => {}
            Judged::Read { head, facts } => {
                let which = format!(
                    "Pull request #{} in {} was opened by {}, not by {}, the account this merge would be made as",
                    facts.number, facts.repo, facts.author, facts.merger
                );
                match head {
                    Head::Unnamed => return Judgement::Refuse(name_the_head(&which)),
                    Head::Named(named) if named.len() < 7 => {
                        return Judgement::Refuse(name_the_head(&format!(
                            "{which}. {named} is too short to say which commit it is"
                        )));
                    }
                    Head::Named(named) if !same_commit(named, &facts.head) => {
                        return Judgement::Refuse(format!(
                            "Swarm refused this merge (ADR 0179). {which}. Its head is now {}, not {named}, so it moved after that commit was chosen, and nothing the operator decided about the old head covers it. Read the new head, and ask again for a command that names it. Do not merge it any other way.",
                            facts.head
                        ));
                    }
                    Head::Named(_) | Head::Hidden => needing.push(which),
                }
            }
            Judged::Unread { what, head } => {
                // Say only what is known: an answer it could not read is not
                // a stranger's pull request or a public repository.
                let which = format!(
                    "This command may merge a pull request, and Swarm could not read who opened it ({what}), so it cannot tell whether the rule covers this merge and holds it as if it did. Try again first"
                );
                if *head == Head::Unnamed {
                    return Judgement::Refuse(name_the_head(&which));
                }
                needing.push(which);
            }
        }
    }
    if needing.is_empty() {
        Judgement::Pass
    } else {
        Judgement::NeedsGrant(needing.join(". "))
    }
}

/// `--match-head-commit` takes a full SHA; a long enough prefix of the head is
/// the same commit, and anything shorter is not trusted to be.
fn same_commit(named: &str, head: &str) -> bool {
    head.get(..named.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(named))
}

fn needs_grant(which: &str) -> String {
    format!(
        "Swarm refused this merge (ADR 0179). {which}. In the operator's own public repositories, merging a pull request that account did not open needs the operator's own decision on that pull request and its head, and neither Queen's go-ahead nor a card Queen grants covers it. To ask, call swarm_request_decision on your task with this exact command in its command field. When the operator presses its grant, run the command again exactly as written. Do not merge it any other way."
    )
}

fn name_the_head(which: &str) -> String {
    format!(
        "Swarm refused this merge (ADR 0179). {which}. A merge of someone else's pull request in the operator's own public repositories must name the commit it merges, so the operator's decision covers that head and nothing pushed after it. Add --match-head-commit with the head's full SHA (for the merge API, the sha field), then ask the operator for that exact command with swarm_request_decision. Do not merge it any other way."
    )
}

fn could_not_check(error: &str) -> String {
    format!(
        "Swarm could not check this merge ({error}), so it is refused (ADR 0179). Try again. If it is still refused and the pull request was opened by someone other than the account this merge would be made as, ask the operator for this exact command with swarm_request_decision. Do not merge it any other way."
    )
}

// ── Reading the command ─────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    /// A word part of which bash computes as it runs the command
    /// (`Lexer::computed`), with the rest of the word as `word`. It `splits`
    /// when a part may be any number of words. It is `as_text` when every
    /// such part is the value of a text flag (`holds_text`). It `leads` when
    /// such a part begins it, so it may begin with anything.
    Printed {
        word: String,
        splits: bool,
        as_text: bool,
        leads: bool,
    },
    /// What runs cannot be told from the text: a value bash reads as
    /// arithmetic or as a name, where a subscript runs commands, or a
    /// function defined, which a name then runs in place of its program.
    Opaque,
    /// Anything that ends one pipeline: `;`, `&`, `&&`, `||`, a newline, a
    /// parenthesis, or the edge of a command substitution.
    Break,
    /// The command this is in may not run, or runs in a shell of its own:
    /// after `&&` or `||`, inside parentheses, a compound command or a
    /// substitution, or in a list run in the background. A `cd` there
    /// leaves the folder untold (ADR 0190).
    Apart,
    /// `|`: the next command reads this one's output.
    Pipe,
    /// Standard input from a file, a here-string or another descriptor.
    Input,
    /// A heredoc's body: what the command it belongs to reads, on standard
    /// input when it is `standard`, given no descriptor or 0 (ADR 0188). It
    /// `expands` when no part of its delimiter was quoted, so its `$(...)`
    /// and backquotes run as the shell reads it.
    Stdin {
        body: String,
        expands: bool,
        standard: bool,
    },
}

impl Token {
    /// A word of a command: for a word bash computes part of, whether that
    /// is only text, one word that is the value of a text flag.
    fn said(&self) -> Option<(&str, Option<bool>)> {
        match self {
            Token::Word(word) => Some((word, None)),
            Token::Printed {
                word,
                splits,
                as_text,
                ..
            } => Some((word, Some(!splits && *as_text))),
            _ => None,
        }
    }
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// Splits shell text into words and command boundaries, as far as deciding
/// what runs needs: quotes, escapes, comments, redirections, heredocs, the
/// parts of a word bash computes, and command substitution, whose inside is
/// read as commands of its own.
///
/// `None` when a quote, a substitution or a heredoc is never closed.
fn tokens(text: &str) -> Option<Vec<Token>> {
    lex(&mut text.chars().peekable(), false)
}

/// Reads commands to the end of the text or, for a `$(` read inside double
/// quotes (`substitution`), to the parenthesis that closes it. Reading the
/// substitution from the same text, rather than cutting it out first, keeps
/// a parenthesis inside its quotes or heredocs from closing it.
fn lex(chars: &mut Chars<'_>, substitution: bool) -> Option<Vec<Token>> {
    let mut lexer = Lexer::default();
    while let Some(character) = chars.next() {
        match character {
            // In a `case` pattern, a `(` before the first may open it.
            '(' if lexer.at_pattern_start() => {}
            '\'' => {
                lexer.started = true;
                lexer.quotes += 1;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        other => lexer.word.push(other),
                    }
                }
            }
            '"' => lexer.double_quoted(chars)?,
            '\\' => match chars.next() {
                Some('\n') | None => {}
                Some(escaped) => {
                    lexer.started = true;
                    lexer.quotes += 1;
                    lexer.word.push(escaped);
                }
            },
            ' ' | '\t' => lexer.finish(),
            '\n' => {
                lexer.list_ends();
                lexer.read_heredocs(chars, substitution)?;
            }
            '|' | '&' if chars.peek() == Some(&character) => {
                chars.next();
                lexer.and_or();
            }
            '|' => {
                // `|&` pipes standard error as well.
                if chars.peek() == Some(&'&') {
                    chars.next();
                }
                lexer.pipe();
            }
            // `&>` and `&>>` send both outputs to a file: a redirection.
            '&' if chars.peek() == Some(&'>') => {
                lexer.finish();
                chars.next();
                lexer.redirection('>', chars)?;
            }
            '&' => lexer.background(),
            // `;;`, `;&` and `;;&` end a `case` clause's commands.
            ';' if lexer.in_clause() && matches!(chars.peek(), Some(';' | '&')) => {
                if chars.next() == Some(';') {
                    chars.next_if_eq(&'&');
                }
                lexer.clause_ends();
            }
            ';' => lexer.list_ends(),
            // With extglob on, `!(`, `@(`, `+(`, `*(` and `?(` start a
            // pattern, which this does not read.
            '(' if lexer.word.ends_with(['!', '@', '+', '*', '?']) => return None,
            // `name=(...)` assigns an array, whose words are words.
            '(' if lexer.assigns_an_array() => lexer.array(chars)?,
            // `(( ... ))` is arithmetic, where `<<` is a shift. When bash
            // reads it otherwise, it is two subshells.
            '(' if chars.peek() == Some(&'(') => {
                lexer.boundary();
                match arithmetic(chars)? {
                    Parenthesis::Arithmetic { length, names } => {
                        chars.nth(length - 1);
                        lexer.evaluates(names);
                    }
                    Parenthesis::Commands => lexer.depth += 1,
                }
            }
            '(' => {
                // `name ()` defines a function, which runs in place of any
                // program of that name: what a name runs is not told. An
                // array's `=()` was read above, so bash reads any other
                // word before `()` as a function's name, `=` and `"a"=`
                // among them.
                if defines_a_function(chars) {
                    lexer.finish();
                    lexer.tokens.push(Token::Opaque);
                }
                lexer.depth += 1;
                lexer.boundary();
            }
            ')' => {
                lexer.finish();
                if lexer.in_pattern() {
                    // A `case` pattern's end, wherever it stands, even
                    // inside a substitution.
                    lexer.pattern_ends();
                } else if substitution && lexer.depth == 0 {
                    lexer.end();
                    return (lexer.heredocs.is_empty() && lexer.closed()).then_some(lexer.tokens);
                } else {
                    lexer.depth = lexer.depth.saturating_sub(1);
                    lexer.boundary();
                }
            }
            '`' => lexer.substitution(tokens(&take_backquoted(chars)?)?, false)?,
            '$' => lexer.dollar(chars)?,
            // `<(` and `>(` start a process substitution wherever they
            // stand, even after digits: bash reads `2>(x)` as a word. Its
            // commands run, as a substitution's do, and the word it is part
            // of is a file's name bash computes.
            '<' | '>' if chars.peek() == Some(&'(') => {
                chars.next();
                lexer.substitution(lex(chars, true)?, false)?;
            }
            '<' | '>' => lexer.redirection(character, chars)?,
            '#' if !lexer.started => {
                while chars.peek().is_some_and(|next| *next != '\n') {
                    chars.next();
                }
            }
            other => lexer.bare(other, chars)?,
        }
    }
    // A substitution the text never closes, a heredoc whose body never
    // came, or a `case`, compound or `[[` left open: what the shell would
    // run cannot be told.
    lexer.end();
    (!substitution && lexer.heredocs.is_empty() && lexer.closed()).then_some(lexer.tokens)
}

/// What bash computes in a word.
#[derive(Clone, Copy)]
struct Printing {
    /// A part of it may be any number of words, so the word splits.
    splits: bool,
    /// Every part of it is the value of a text flag.
    as_text: bool,
    /// Such a part begins it: what it begins with, a `-` among others, is
    /// not in the text.
    leads: bool,
}

impl Printing {
    /// One word that begins as written, other than with `-`: neither more
    /// words nor a flag, whatever bash computes in it.
    fn one_operand(self, word: &str) -> bool {
        !self.splits && !self.leads && !word.starts_with('-')
    }
}

#[derive(Default)]
struct Lexer {
    tokens: Vec<Token>,
    word: String,
    /// A word has begun, even an empty quoted one.
    started: bool,
    /// How many quoted or escaped parts the word holds: with one, it
    /// names no descriptor before a redirection.
    quotes: usize,
    /// Bash computes part of the word.
    printed: Option<Printing>,
    /// The `{` outside quotes in the word that no `}` has closed: a `,` or
    /// `..` in one makes a list bash expands.
    braces: usize,
    /// How many `[` outside quotes the word holds: a `]` after one makes a
    /// glob.
    brackets: usize,
    /// How far the word so far is an assignment bash reads.
    naming: Naming,
    /// Redirections have been read since the command's last word, and
    /// whether an assignment may stand after them: only where nothing but
    /// redirections came before them in the command.
    redirected: Option<bool>,
    /// How deep the text being read is in `${...}`, `$[...]` and
    /// subscripts (`Lexer::matched`).
    nesting: usize,
    /// How bash reads the text being read in a `${...}`
    /// (`Lexer::expansion`).
    word_text: WordText,
    /// What the next word is when it is not part of the command.
    target: Target,
    /// Heredocs on the current line, in order: their bodies start after the
    /// next newline.
    heredocs: Vec<Heredoc>,
    /// The commands of this command's substitutions, which follow it once
    /// it ends, so that its own words stay together.
    substituted: Vec<Token>,
    /// Parentheses open around the command being read.
    depth: usize,
    /// Compound commands open around it: `if`, `while`, `until`, `for`,
    /// `select`, `case` and `{`.
    compounds: usize,
    /// It follows `&&` or `||` in its list, so it may not run.
    and_or: bool,
    /// Where the list being read began: a `&` after it runs all of it in
    /// the background.
    list_start: usize,
    /// The `case` commands open around it, innermost last.
    cases: Vec<Case>,
    /// What bash reads after the last word, when a word was the last thing
    /// read (`Lexer::position`).
    follows: Follows,
    /// Inside `[[ ... ]]`, whose words are no commands.
    conditional: bool,
}

/// How bash reads the text of a `${...}` (`Lexer::expansion`).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum WordText {
    /// As it reads a word: a `'` is a quote.
    #[default]
    Word,
    /// As double-quoted text, the word of a default, an alternative, an
    /// assigned value or an error message in double quotes: a `'` is a
    /// letter, or in posix mode and in dash one whose `}` ends the
    /// `${...}`.
    DoubleQuoted,
}

/// How far the word being read is an assignment bash reads.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Naming {
    /// None, or not yet.
    #[default]
    Unnamed,
    /// A name as written where an assignment may stand and the subscript
    /// bash read after it whole (`Lexer::subscript`): this many bytes.
    Subscript(usize),
    /// `name=`, `name+=` or `name[...]=`, its name as written, where an
    /// assignment may stand: bash reads the word as one.
    Assignment,
}

/// A word after a redirection, which is no part of the command.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Target {
    /// The next word is part of the command.
    #[default]
    None,
    /// A redirection's file.
    File,
    /// A here-string's word, what the command reads, on standard input
    /// when `true`.
    HereString(bool),
}

/// What bash reads after a word, as far as reserved words go: where a word
/// is reserved, bash reads it so only where it reads a command.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Follows {
    /// A command: after a reserved word in `RESERVED` read as one, after
    /// `time`'s `--`, and after a compound's close, `fi`, `done`, `}`,
    /// `esac` or `]]`, where an outer compound's next word may come.
    #[default]
    Command,
    /// After `time`: `-p`, `--`, or a command.
    Time,
    /// After `time -p`: `--`, or a command.
    TimeOption,
    /// After `coproc`: a compound command, or a name or a simple command.
    Coproc,
    /// After `coproc` and a word: a compound's opener, which makes that
    /// word the coprocess's name, is the only word reserved here
    /// (`opens_a_coprocess`).
    Named,
    /// After `for` or `select`: its name.
    Name,
    /// After `for` or `select` and its name, where `do` is reserved.
    List,
    /// After an assignment where one may stand: another assignment or a
    /// command, where no word is reserved.
    Assigned,
    /// After an argument, a name or any other word.
    Argument,
}

/// Where bash is in reading a `case` command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    /// After `case`: the word it matches.
    Subject,
    /// After that word: `in`.
    In,
    /// Reading patterns, `start` before the first of a clause, where
    /// `esac` ends the command.
    Pattern { start: bool },
    /// Reading a clause's commands, after its pattern's `)`.
    Clause,
}

/// What a word is to a `case` command around it.
#[derive(PartialEq, Eq)]
enum CaseWord {
    /// One of its patterns.
    Pattern,
    /// The word it matches, `in`, or the `esac` that ends it before any
    /// clause.
    Grammar,
    /// A word of a command.
    Other,
}

/// Reserved words that open a compound command, and those that close one.
const OPENERS: &[&str] = &["if", "while", "until", "for", "select", "{"];
const CLOSERS: &[&str] = &["fi", "done", "}"];

/// Whether `word`, after `coproc` and a name, opens the compound command
/// that makes the name the coprocess's: `case`, `[[` or an opener. `((`
/// is read as arithmetic wherever it stands.
fn opens_a_coprocess(word: &str) -> bool {
    word == "case" || word == "[[" || OPENERS.contains(&word)
}

struct Heredoc {
    delimiter: String,
    /// `<<-`: leading tabs are taken off each line and the delimiter's.
    strip_tabs: bool,
    /// Any quoting in the delimiter keeps the body as written. Unquoted, its
    /// `$(...)` and backquotes run as the heredoc is read.
    quoted: bool,
    /// On standard input: no descriptor before `<<`, or 0.
    standard: bool,
    /// Where its `Token::Stdin` waits for the body.
    at: usize,
}

/// Whether a word right before a redirection may be `{name[subscript]}`,
/// an array element bash sets to a new descriptor. Which subscripts bash
/// takes is its own parser's to say, so any braced word holding a `[` may
/// be one (ADR 0188).
fn may_name_an_element(word: &str) -> bool {
    word.starts_with('{') && word.ends_with('}') && word.contains('[')
}

/// Whether a word right before a redirection is a `{name}`, for which bash
/// opens a new descriptor and sets the variable `name` to it.
fn names_a_descriptor(word: &str) -> bool {
    word.strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .is_some_and(|name| {
            name.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
                && name
                    .chars()
                    .all(|letter| letter.is_ascii_alphanumeric() || letter == '_')
        })
}

impl Lexer {
    /// The rest of a double-quoted string, whose opening quote has been read.
    fn double_quoted(&mut self, chars: &mut Chars<'_>) -> Option<()> {
        self.started = true;
        self.quotes += 1;
        loop {
            match chars.next()? {
                '"' => return Some(()),
                '\\' => {
                    let escaped = chars.next()?;
                    if !matches!(escaped, '"' | '\\' | '$' | '`' | '\n') {
                        self.word.push('\\');
                    }
                    if escaped != '\n' {
                        self.word.push(escaped);
                    }
                }
                // The rest of the quote is still one word.
                '`' => self.substitution(tokens(&take_backquoted(chars)?)?, true)?,
                '$' if chars.next_if_eq(&'(').is_some() => match dollar_parenthesis(chars)? {
                    Dollar::Substitution(inner) => self.substitution(inner, true)?,
                    Dollar::Arithmetic { names } => self.evaluates(names),
                },
                '$' if chars.peek() == Some(&'[') => {
                    self.evaluates(true);
                    self.computed(true);
                    self.word.push('$');
                    self.expansion(chars, true)?;
                }
                '$' if chars.peek() == Some(&'{') => {
                    self.evaluates(evaluates_a_value(chars));
                    self.computed(!quoted_words(chars));
                    self.word.push('$');
                    self.expansion(chars, true)?;
                }
                '$' if chars.peek().copied().is_some_and(parameter) => {
                    self.computed(!quoted_words(chars));
                    self.word.push('$');
                }
                other => self.word.push(other),
            }
        }
    }

    /// The rest of a `$'...'` string, whose opening has been read: quoted
    /// text, which bash makes as it reads the command (`ansi_c_text`).
    fn ansi_c_quoted(&mut self, chars: &mut Chars<'_>) -> Option<()> {
        self.started = true;
        self.quotes += 1;
        let body = ansi_c_body(chars)?;
        self.word.push_str(&ansi_c_text(&body));
        Some(())
    }

    /// At a `(`: whether the word before it is `name=`, `name+=` or
    /// `name[...]=` with its name as written, which bash reads with the `(`
    /// as an array. A name bash computes keeps its `$` in the word.
    fn assigns_an_array(&self) -> bool {
        self.quotes == 0
            && self
                .word
                .strip_suffix('=')
                .map(|name| name.strip_suffix('+').unwrap_or(name))
                .and_then(|name| match name.split_once('[') {
                    Some((name, subscript)) => subscript.ends_with(']').then_some(name),
                    None => Some(name),
                })
                .is_some_and(is_name)
    }

    /// The rest of an array, `name=(...)`, whose `(` has been read: its
    /// words, read into the assignment's word as bash reads them, to the
    /// `)` that ends it. None of them is reserved, and a substitution among
    /// them runs. A comment runs to the line's end. A `(`, a `;`, a `|`, a
    /// `&` or a redirection is a syntax error, and `None` (ADR 0190).
    fn array(&mut self, chars: &mut Chars<'_>) -> Option<()> {
        self.word.push('(');
        let mut gap = true;
        loop {
            let character = chars.next()?;
            let after_gap = std::mem::replace(&mut gap, false);
            match character {
                ')' => {
                    self.word.push(')');
                    return Some(());
                }
                // A heredoc's body would start on the next line.
                '\n' if !self.heredocs.is_empty() => return None,
                ' ' | '\t' | '\n' => {
                    if !after_gap {
                        self.word.push(' ');
                    }
                    gap = true;
                }
                '#' if after_gap => {
                    while chars.next_if(|next| *next != '\n').is_some() {}
                    gap = true;
                }
                '\'' => {
                    self.quotes += 1;
                    loop {
                        match chars.next()? {
                            '\'' => break,
                            other => self.word.push(other),
                        }
                    }
                }
                '"' => self.double_quoted(chars)?,
                '\\' => match chars.next()? {
                    '\n' => {}
                    escaped => {
                        self.quotes += 1;
                        self.word.push(escaped);
                    }
                },
                '$' => self.dollar(chars)?,
                '`' => self.substitution(tokens(&take_backquoted(chars)?)?, false)?,
                '<' | '>' if chars.peek() == Some(&'(') => {
                    chars.next();
                    self.substitution(lex(chars, true)?, false)?;
                }
                '(' | ';' | '|' | '&' | '<' | '>' => return None,
                // Bash reads a subscript that begins a word whole.
                '[' if after_gap => self.subscript(chars)?,
                other => self.unquoted(other, chars.peek().copied()),
            }
        }
    }

    /// A character outside quotes, as `unquoted` reads it, but for a `[`
    /// after a name where bash reads a subscript (`subscript_may_follow`).
    fn bare(&mut self, character: char, chars: &mut Chars<'_>) -> Option<()> {
        if character == '[' && self.subscript_may_follow() {
            self.subscript(chars)?;
            self.naming = Naming::Subscript(self.word.len());
        } else {
            self.unquoted(character, chars.peek().copied());
        }
        Some(())
    }

    /// A character of the word outside quotes, before `next`. A glob, a
    /// brace list or a leading `~` is part of the word bash computes.
    fn unquoted(&mut self, character: char, next: Option<char>) {
        let computed = match character {
            '*' | '?' => Some(false),
            '[' => {
                self.brackets += 1;
                None
            }
            ']' if self.brackets > 0 => Some(false),
            '{' => {
                self.braces += 1;
                None
            }
            '}' => {
                self.braces = self.braces.saturating_sub(1);
                None
            }
            ',' if self.braces > 0 => Some(false),
            '.' if self.braces > 0 && next == Some('.') => Some(false),
            '~' if !self.started => Some(true),
            _ => None,
        };
        if let Some(whole) = computed {
            self.computed(whole);
        }
        if character == '=' && self.naming != Naming::Assignment {
            let name = self.word.strip_suffix('+').unwrap_or(&self.word);
            let assigns = self.quotes == 0
                && match self.naming {
                    Naming::Subscript(end) => name.len() == end,
                    _ => self.printed.is_none() && is_name(name) && self.assignment_may_stand(),
                };
            if assigns {
                self.naming = Naming::Assignment;
            }
        }
        self.started = true;
        self.word.push(character);
    }

    /// An expansion that reads a value as arithmetic or as a name, where a
    /// subscript in that value runs commands: what runs cannot be told.
    fn evaluates(&mut self, names: bool) {
        if names {
            self.tokens.push(Token::Opaque);
        }
    }

    /// A `$` outside quotes, which has been read: a substitution, a value
    /// bash computes, some forms of which read a value as arithmetic or as
    /// a name, quoted text, or a `$` as it is.
    fn dollar(&mut self, chars: &mut Chars<'_>) -> Option<()> {
        if chars.next_if_eq(&'(').is_some() {
            self.started = true;
            match dollar_parenthesis(chars)? {
                Dollar::Substitution(inner) => self.substitution(inner, false)?,
                Dollar::Arithmetic { names } => self.evaluates(names),
            }
            return Some(());
        }
        if chars.next_if_eq(&'\'').is_some() {
            return self.ansi_c_quoted(chars);
        }
        match chars.peek().copied() {
            Some('[' | '{') => {
                self.evaluates(evaluates_a_value(chars));
                self.computed(false);
                self.word.push('$');
                return self.expansion(chars, false);
            }
            // `$"..."` is the quoted text as translated.
            Some('"') => {
                self.computed(true);
            }
            Some(next) if parameter(next) => {
                self.computed(false);
            }
            next => {
                self.unquoted('$', next);
                return Some(());
            }
        }
        self.word.push('$');
        Some(())
    }

    /// Whether `word` stands where bash reads a command, so a reserved word
    /// there is one: at the start, after a list's or a pipeline's end, or
    /// after a word bash reads as reserved itself (`Follows`). A reserved
    /// word given as an argument, as `echo if`, is a word. Inside `[[ ... ]]`
    /// none is.
    fn at_command_start(&self, word: &str) -> bool {
        !self.conditional
            && match self.position() {
                Follows::Command | Follows::Time | Follows::TimeOption | Follows::Coproc => true,
                Follows::Named => opens_a_coprocess(word),
                Follows::List => word == "do",
                Follows::Name | Follows::Assigned | Follows::Argument => false,
            }
    }

    /// Whether bash reads an assignment where the word being read stands,
    /// and so a subscript after a name whole: where it reads a command,
    /// after assignments there, and after redirections that begin a
    /// command. Not in a `case` command's subject or patterns, nor inside
    /// `[[ ... ]]`.
    fn assignment_may_stand(&self) -> bool {
        self.redirected
            .unwrap_or_else(|| self.after_a_command_start(true))
    }

    /// Whether the last word read leaves bash reading a command, or, with
    /// `assigned`, an assignment before one.
    fn after_a_command_start(&self, assigned: bool) -> bool {
        !self.conditional
            && matches!(self.cases.last(), None | Some(Case::Clause))
            && match self.position() {
                Follows::Command
                | Follows::Time
                | Follows::TimeOption
                | Follows::Coproc
                | Follows::Named => true,
                Follows::Assigned => assigned,
                Follows::List | Follows::Name | Follows::Argument => false,
            }
    }

    /// At a `[` outside quotes: whether the word before it is a name as
    /// written where an assignment may stand, so bash reads a subscript.
    fn subscript_may_follow(&self) -> bool {
        self.target == Target::None
            && self.quotes == 0
            && self.printed.is_none()
            && is_name(&self.word)
            && self.assignment_may_stand()
    }

    /// A subscript, whose `[` has been read: bash reads it to the `]` that
    /// matches it as one part of the word, spaces, `;`, `|`, `&`, `#`,
    /// parentheses and newlines among it. Its quotes are no part of the
    /// word's own. As a glob, bash may expand the word (ADR 0190).
    fn subscript(&mut self, chars: &mut Chars<'_>) -> Option<()> {
        let quotes = self.quotes;
        self.word.push('[');
        self.matched(chars, ']')?;
        self.word.push(']');
        self.quotes = quotes;
        self.computed(false);
        Some(())
    }

    /// Text bash reads to the `close` that ends it, `]` or `}`, whose
    /// opening has been read: a subscript, a `$[...]` or a `${...}`. A `[`
    /// in a subscript or a `$[...]` nests. In any, a quote, a backslash, a
    /// `$(...)`, a backquoted command and a `${...}` or `$[...]` of its own
    /// are read as bash reads them, and a substitution among them runs.
    /// Text nested deeper than `MAX_NESTING` is not read, nor a `'` or a
    /// `$'` in a word bash expands as double-quoted text (`WordText`):
    /// bash runs a substitution between two `'` there, and in posix mode
    /// and in dash the first `}` ends the `${...}` even between them.
    fn matched(&mut self, chars: &mut Chars<'_>, close: char) -> Option<()> {
        if self.nesting == MAX_NESTING {
            return None;
        }
        self.nesting += 1;
        let mut depth = 0_usize;
        loop {
            let character = chars.next()?;
            match character {
                _ if character == close && depth == 0 => break,
                _ if character == close => depth -= 1,
                '[' if close == ']' => depth += 1,
                '\\' => {
                    match chars.next()? {
                        '\n' => {}
                        escaped => self.word.push(escaped),
                    }
                    continue;
                }
                '\'' if self.word_text == WordText::DoubleQuoted => return None,
                '$' if self.word_text == WordText::DoubleQuoted && chars.peek() == Some(&'\'') => {
                    return None;
                }
                '\'' => {
                    loop {
                        match chars.next()? {
                            '\'' => break,
                            other => self.word.push(other),
                        }
                    }
                    continue;
                }
                '"' => {
                    self.double_quoted(chars)?;
                    continue;
                }
                '`' => {
                    self.substitution(tokens(&take_backquoted(chars)?)?, false)?;
                    continue;
                }
                '$' => {
                    self.dollar(chars)?;
                    continue;
                }
                _ => {}
            }
            self.word.push(character);
        }
        self.nesting -= 1;
        Some(())
    }

    /// The rest of a `${...}` or `$[...]`, whose `$` has been read and
    /// pushed: bash reads it whole, to the brace or bracket that matches
    /// its own (`matched`). In double quotes, `quoted`, nothing in it splits
    /// the word, and its word is double-quoted text unless it is a pattern
    /// or a change of case, as in a word of one around it that is.
    fn expansion(&mut self, chars: &mut Chars<'_>, quoted: bool) -> Option<()> {
        let open = chars.next()?;
        let close = if open == '[' { ']' } else { '}' };
        let printed = self.printed;
        let word_text =
            if (quoted || self.word_text == WordText::DoubleQuoted) && !keeps_its_quotes(chars) {
                WordText::DoubleQuoted
            } else {
                WordText::Word
            };
        let around = std::mem::replace(&mut self.word_text, word_text);
        self.word.push(open);
        self.matched(chars, close)?;
        self.word_text = around;
        self.word.push(close);
        if quoted {
            self.printed = printed;
        }
        Some(())
    }

    /// What bash reads where the word now read stands, from what was read
    /// last: a command after a list's or a pipeline's end, and after a
    /// word what that word left (`follows`).
    fn position(&self) -> Follows {
        match self
            .tokens
            .iter()
            .rev()
            .find(|token| **token != Token::Apart)
        {
            None | Some(Token::Break | Token::Pipe) => Follows::Command,
            Some(Token::Word(_)) => self.follows,
            // A word bash computes a part of is reserved nowhere, but it may
            // be an assignment, as `x=$y` is.
            Some(Token::Printed { .. }) if self.follows == Follows::Assigned => Follows::Assigned,
            Some(_) => Follows::Argument,
        }
    }

    /// What bash reads after `word`, read at `at`, where `starts` says it
    /// stands where bash reads a command, as written, and `plain` that no
    /// part of it is quoted or computed.
    fn after(
        &mut self,
        at: Follows,
        word: &str,
        read: &CaseWord,
        plain: bool,
        starts: bool,
    ) -> Follows {
        if self.conditional {
            if plain && word == "]]" {
                self.conditional = false;
                return Follows::Command;
            }
            return Follows::Argument;
        }
        if *read == CaseWord::Grammar {
            return if word == "esac" {
                Follows::Command
            } else {
                Follows::Argument
            };
        }
        match word {
            _ if !starts => match at {
                Follows::Name => Follows::List,
                _ => Follows::Argument,
            },
            "time" => Follows::Time,
            "-p" if at == Follows::Time => Follows::TimeOption,
            "--" if matches!(at, Follows::Time | Follows::TimeOption) => Follows::Command,
            "coproc" => Follows::Coproc,
            "for" | "select" => Follows::Name,
            "[[" => {
                self.conditional = true;
                Follows::Argument
            }
            _ if RESERVED.contains(&word) || CLOSERS.contains(&word) || word == "esac" => {
                Follows::Command
            }
            _ if at == Follows::Coproc => Follows::Named,
            _ => Follows::Argument,
        }
    }

    /// Whether no `case`, compound command or `[[` is left open, as bash
    /// needs where the text ends: one left open is a syntax error.
    fn closed(&self) -> bool {
        self.cases.is_empty() && self.compounds == 0 && !self.conditional
    }

    /// Whether this is reading a `case` command's patterns.
    fn in_pattern(&self) -> bool {
        matches!(self.cases.last(), Some(Case::Pattern { .. }))
    }

    /// Whether this is before the first pattern of a clause, with no word
    /// begun.
    fn at_pattern_start(&self) -> bool {
        self.cases.last() == Some(&Case::Pattern { start: true }) && !self.started
    }

    /// Whether this is reading the commands of a `case` clause.
    fn in_clause(&self) -> bool {
        self.cases.last() == Some(&Case::Clause)
    }

    /// A pattern's `)`: the clause's commands follow.
    fn pattern_ends(&mut self) {
        if let Some(case) = self.cases.last_mut() {
            *case = Case::Clause;
        }
        self.boundary();
    }

    /// `;;`, `;&` or `;;&`: the next clause's patterns follow.
    fn clause_ends(&mut self) {
        self.boundary();
        if let Some(case) = self.cases.last_mut() {
            *case = Case::Pattern { start: true };
        }
    }

    /// Whether a command read now may not run in this shell (`Token::Apart`).
    fn uncertain(&self) -> bool {
        self.depth > 0 || self.compounds > 0 || self.and_or
    }

    /// `;` or a newline: a list ends, and whatever ran before, the next
    /// command runs.
    fn list_ends(&mut self) {
        self.and_or = false;
        self.boundary();
        self.list_start = self.tokens.len();
    }

    /// `&&` or `||`: the next command runs only on how this one ends.
    fn and_or(&mut self) {
        self.and_or = true;
        self.boundary();
    }

    /// `&`: the list that ends here runs in the background, in a shell of
    /// its own. Its commands after `&&` or `||` are apart already; its
    /// first is marked here.
    fn background(&mut self) {
        self.finish();
        let start = self.list_start.min(self.tokens.len());
        if self.tokens.get(start) != Some(&Token::Apart) {
            self.tokens.insert(start, Token::Apart);
            for heredoc in &mut self.heredocs {
                if heredoc.at >= start {
                    heredoc.at += 1;
                }
            }
        }
        self.list_ends();
    }

    /// A redirection, whose first `<` or `>` has been read. Its file is not
    /// a word of the command; a heredoc's body is what the command reads.
    fn redirection(&mut self, first: char, chars: &mut Chars<'_>) -> Option<()> {
        // `2>` and `{fd}>` name a descriptor, not a word of the command, when
        // no quote or backslash is in them. A heredoc is standard input only
        // on descriptor 0, which it takes unnamed. Anything else put onto
        // descriptor 0 by name, as `0>&3` puts back what `3>&0` saved, is
        // standard input the gate does not follow.
        let mut standard = true;
        let mut onto_input = false;
        let plain = self.started && self.printed.is_none() && self.quotes == 0;
        if plain && self.word.chars().all(|digit| digit.is_ascii_digit()) {
            standard = self.word.chars().all(|digit| digit == '0');
            onto_input = standard && !self.word.is_empty();
            self.word.clear();
            self.started = false;
        } else if plain && names_a_descriptor(&self.word) {
            standard = false;
            self.word.clear();
            self.started = false;
        } else if may_name_an_element(&self.word) {
            // bash may take it as a descriptor or as a word of the command,
            // and the gate cannot tell which.
            return None;
        } else {
            self.finish();
        }
        if self.redirected.is_none() {
            self.redirected = Some(self.after_a_command_start(false));
        }
        if first == '<' && chars.peek() == Some(&'<') {
            chars.next();
            if chars.peek() != Some(&'<') {
                let strip_tabs = chars.next_if_eq(&'-').is_some();
                let (delimiter, quoted) = heredoc_delimiter(chars)?;
                self.heredoc(delimiter, strip_tabs, quoted, standard);
                return Some(());
            }
            // A here-string: its word, and a newline, are the input.
            chars.next();
            self.target = Target::HereString(standard);
            return Some(());
        }
        // `>|` writes over a file even with noclobber set.
        if first == '>' {
            chars.next_if_eq(&'|');
        }
        while matches!(chars.peek(), Some('<' | '>')) {
            chars.next();
        }
        if first == '<' || onto_input {
            self.tokens.push(Token::Input);
        }
        // `>&2` and `<&-` dup or close a descriptor. `>&` before anything
        // else sends both outputs to the file it names, as `&>` does.
        if chars.next_if_eq(&'&').is_some()
            && chars
                .peek()
                .is_some_and(|next| next.is_ascii_digit() || *next == '-')
        {
            while chars
                .peek()
                .is_some_and(|next| next.is_ascii_digit() || *next == '-')
            {
                chars.next();
            }
            return Some(());
        }
        self.target = Target::File;
        Some(())
    }

    fn pipe(&mut self) {
        self.finish();
        self.redirected = None;
        self.tokens.push(Token::Pipe);
        if self.uncertain() {
            self.tokens.push(Token::Apart);
        }
    }

    fn heredoc(&mut self, delimiter: String, strip_tabs: bool, quoted: bool, standard: bool) {
        self.heredocs.push(Heredoc {
            delimiter,
            strip_tabs,
            quoted,
            standard,
            at: self.tokens.len(),
        });
        self.tokens.push(Token::Stdin {
            body: String::new(),
            expands: false,
            standard,
        });
    }

    /// The bodies of this line's heredocs, which start after its newline.
    /// `in_substitution` says this reads the inside of a `$(`, where bash
    /// ends a heredoc early.
    fn read_heredocs(&mut self, chars: &mut Chars<'_>, in_substitution: bool) -> Option<()> {
        for heredoc in std::mem::take(&mut self.heredocs) {
            self.tokens[heredoc.at] = Token::Stdin {
                body: heredoc_body(chars, &heredoc, in_substitution)?,
                expands: !heredoc.quoted,
                standard: heredoc.standard,
            };
        }
        Some(())
    }

    fn finish(&mut self) {
        self.braces = 0;
        self.brackets = 0;
        let assigns = std::mem::take(&mut self.naming) == Naming::Assignment;
        let plain = std::mem::take(&mut self.quotes) == 0;
        if self.started {
            let word = std::mem::take(&mut self.word);
            let printed = self.printed.take();
            if self.target == Target::None {
                let plain = plain && printed.is_none();
                let read = self.read_as_case(&word, plain);
                if read == CaseWord::Pattern {
                    // A pattern is not a word of any command. One that names
                    // gh, curl or a merge may be a command read wrongly as
                    // one, so in a command that mentions a merge it is not
                    // read (ADR 0190).
                    if ["gh", "curl", "merge"]
                        .iter()
                        .any(|name| names_a_word(&word, name))
                    {
                        self.tokens.push(Token::Opaque);
                    }
                    self.started = false;
                    return;
                }
                let at = self.position();
                let starts = read == CaseWord::Other && plain && self.at_command_start(&word);
                let opens = starts && self.compound(&word);
                self.follows = if assigns {
                    Follows::Assigned
                } else {
                    self.after(at, &word, &read, plain, starts)
                };
                self.redirected = None;
                self.tokens.push(match printed {
                    Some(Printing {
                        splits,
                        as_text,
                        leads,
                    }) => Token::Printed {
                        word,
                        splits,
                        as_text,
                        leads,
                    },
                    None => Token::Word(word),
                });
                if opens {
                    self.tokens.push(Token::Apart);
                }
            } else {
                // A here-string bash computes no part of is input the gate
                // sees, as a quoted heredoc's body is. Any other is input it
                // cannot see (ADR 0190).
                match std::mem::take(&mut self.target) {
                    Target::HereString(standard) if printed.is_none() => {
                        self.tokens.push(Token::Stdin {
                            body: word,
                            expands: false,
                            standard,
                        });
                    }
                    Target::HereString(_) => self.tokens.push(Token::Input),
                    Target::File | Target::None => {}
                }
            }
            self.started = false;
        }
    }

    /// What a word is to the `case` being read. `plain` is a word with no
    /// quote and nothing bash computes, the only kind bash reads as
    /// reserved.
    fn read_as_case(&mut self, word: &str, plain: bool) -> CaseWord {
        let next = match self.cases.last().copied() {
            Some(Case::Subject) => Case::In,
            Some(Case::In) if plain && word == "in" => Case::Pattern { start: true },
            // Not a `case` bash reads; its words are read as commands.
            Some(Case::In) => Case::Clause,
            Some(Case::Pattern { start: true }) if plain && word == "esac" => {
                self.cases.pop();
                self.compounds = self.compounds.saturating_sub(1);
                return CaseWord::Grammar;
            }
            Some(Case::Pattern { .. }) => {
                if let Some(case) = self.cases.last_mut() {
                    *case = Case::Pattern { start: false };
                }
                return CaseWord::Pattern;
            }
            Some(Case::Clause) | None => return CaseWord::Other,
        };
        if let Some(case) = self.cases.last_mut() {
            *case = next;
        }
        CaseWord::Grammar
    }

    /// For a word where a command starts: whether it opens a compound
    /// command, which it counts. One that closes one is counted off.
    fn compound(&mut self, word: &str) -> bool {
        match word {
            "case" => self.cases.push(Case::Subject),
            "esac" if self.in_clause() => {
                self.cases.pop();
                self.compounds = self.compounds.saturating_sub(1);
                return false;
            }
            _ if CLOSERS.contains(&word) => {
                self.compounds = self.compounds.saturating_sub(1);
                return false;
            }
            _ if !OPENERS.contains(&word) => return false,
            _ => {}
        }
        self.compounds += 1;
        true
    }

    /// The end of a pipeline, followed by the commands of its substitutions.
    fn boundary(&mut self) {
        self.finish();
        self.redirected = None;
        self.tokens.push(Token::Break);
        self.tokens.append(&mut self.substituted);
        if self.uncertain() {
            self.tokens.push(Token::Apart);
        }
    }

    /// The end of the text being read, or of the substitution it is.
    fn end(&mut self) {
        self.finish();
        if !self.substituted.is_empty() {
            self.tokens.push(Token::Break);
            self.tokens.append(&mut self.substituted);
        }
    }

    /// The commands inside a substitution run too, so they are read as
    /// commands of their own once the command around them ends. What the
    /// substitution prints becomes part of the word being read. A heredoc in
    /// it is data only as the text of a gh or git message, title or body,
    /// which is all that word can be there. Anywhere else, what it prints may
    /// be words a command runs, so the heredoc is read as commands here
    /// (ADR 0188).
    fn substitution(&mut self, inner: Vec<Token>, quoted: bool) -> Option<()> {
        let text = self.computed(quoted);
        // Each of its commands runs in a shell of its own.
        let apart = |found: &mut Vec<Token>, tokens: Vec<Token>| {
            found.push(Token::Apart);
            for token in tokens {
                let ends = matches!(token, Token::Break | Token::Pipe);
                found.push(token);
                if ends {
                    found.push(Token::Apart);
                }
            }
        };
        let mut found = Vec::new();
        let mut commands = Vec::new();
        for token in inner {
            match token {
                Token::Stdin { body, .. } if !text => {
                    commands.push(Token::Break);
                    commands.extend(tokens(&body)?);
                    commands.push(Token::Break);
                }
                other => commands.push(other),
            }
        }
        apart(&mut found, commands);
        self.substituted.append(&mut found);
        self.substituted.push(Token::Break);
        Some(())
    }

    /// Bash computes the part of the word now read as it runs the command:
    /// what a substitution prints, a variable, `~`, `$"..."`, a brace list
    /// or a glob. It may be any word, so for gh and git it counts
    /// only as a text flag's value (`holds_text`), and only when it stays
    /// `whole`, one word. Returns whether it is that value (ADR 0188).
    fn computed(&mut self, whole: bool) -> bool {
        let slot = self.in_text_flag();
        let before = self.printed.unwrap_or(Printing {
            splits: false,
            as_text: true,
            leads: self.word.is_empty(),
        });
        self.started = true;
        self.printed = Some(Printing {
            splits: before.splits || !whole,
            as_text: before.as_text && slot,
            leads: before.leads,
        });
        whole && slot
    }

    /// Whether the word being read, as written up to the part now read, is
    /// the value of a gh or git flag whose value is only text.
    fn in_text_flag(&self) -> bool {
        let start = self
            .tokens
            .iter()
            .rposition(|token| matches!(token, Token::Break | Token::Pipe))
            .map_or(0, |at| at + 1);
        let command: Vec<(&str, Option<bool>)> = self.tokens[start..]
            .iter()
            .filter_map(Token::said)
            .collect();
        holds_text(&command, &self.word)
    }
}

/// The flags of gh and git whose value is a message, title or body.
const GH_TEXT_FLAGS: &[&str] = &[
    "--body",
    "-b",
    "--title",
    "-t",
    "--notes",
    "-n",
    "--subject",
];
const GIT_TEXT_FLAGS: &[&str] = &["-m", "--message"];
/// The gh commands those flags give a pull request's, issue's or release's
/// text to, and the git commands that take `-m` as a message.
const GH_TEXT_COMMANDS: &[(&str, &str)] = &[
    ("pr", "create"),
    ("pr", "edit"),
    ("pr", "comment"),
    ("pr", "review"),
    ("pr", "merge"),
    ("issue", "create"),
    ("issue", "edit"),
    ("issue", "comment"),
    ("release", "create"),
    ("release", "edit"),
];
const GIT_TEXT_COMMANDS: &[&str] = &["commit", "tag", "notes"];
/// Flags of those commands that take no value in any of them, so the word
/// after one is read as a flag. gh's `-a`, `-m`, `-p` and `-r` are not here:
/// each takes a value in `pr create` and none in another of these commands.
const GH_SWITCHES: &[&str] = &[
    "--draft",
    "-d",
    "--fill",
    "-f",
    "--web",
    "-w",
    "--dry-run",
    "--prerelease",
    "--latest",
    "--generate-notes",
    "--verify-tag",
    "--edit-last",
    "--approve",
    "--request-changes",
    "--comment",
    "--squash",
    "--rebase",
    "--merge",
    "--delete-branch",
    "--admin",
    "--auto",
];
const GIT_SWITCHES: &[&str] = &[
    "-a",
    "--all",
    "--amend",
    "--allow-empty",
    "--no-verify",
    "-s",
    "--signoff",
    "-q",
    "--quiet",
    "-f",
    "--force",
    "--no-edit",
];

/// A gh or git command that takes text: where its flags begin, its text
/// flags, and its flags that take no value.
struct TextCommand {
    start: usize,
    flags: &'static [&'static str],
    switches: &'static [&'static str],
}

/// The command a gh or git line runs, when it is one that takes text and is
/// named as written. Any other command may take its flags' values in ways
/// the gate does not know, so none of its flags holds only text.
fn text_command(command: &[(&str, Option<bool>)]) -> Option<TextCommand> {
    let Some((&(program, None), arguments)) = command.split_first() else {
        return None;
    };
    match system_name(program)? {
        "gh" => match arguments {
            [(noun, None), (verb, None), ..] if GH_TEXT_COMMANDS.contains(&(*noun, *verb)) => {
                Some(TextCommand {
                    start: 3,
                    flags: GH_TEXT_FLAGS,
                    switches: GH_SWITCHES,
                })
            }
            _ => None,
        },
        "git" => git_command(arguments)
            .filter(|&at| GIT_TEXT_COMMANDS.contains(&arguments[at].0))
            .map(|at| TextCommand {
                start: at + 2,
                flags: GIT_TEXT_FLAGS,
                switches: GIT_SWITCHES,
            }),
        _ => None,
    }
}

/// Whether a word being read, `word` as written so far, is the value of a
/// text flag of the command whose words before it are `command`: in gh a
/// pull request's, issue's or release's text, and in git a commit's, tag's
/// or note's message. The flag counts only where gh or git reads it as a
/// flag, not as the value of the flag before it, as `-b` is after
/// `--template` (ADR 0188).
fn holds_text(command: &[(&str, Option<bool>)], word: &str) -> bool {
    let Some(text) = text_command(command) else {
        return false;
    };
    let at = match word.split_once('=') {
        Some((flag, _)) if flag.starts_with("--") => {
            text.flags.contains(&flag).then_some(command.len())
        }
        _ => match command.split_last() {
            Some((&(flag, None), _)) if text.flags.contains(&flag) => Some(command.len() - 1),
            _ => None,
        },
    };
    at.and_then(|at| command.get(text.start..at))
        .is_some_and(|before| read_as_flag(before, text.switches))
}

/// Whether gh or git reads the word after `before`, the words from its
/// command's first flag on, as a flag. Nothing before it may end the flags
/// or be a computed word that can be any word, and the word just before it
/// must take no value: a word that is not a flag, a flag with its value
/// attached, a flag that takes none, or a text flag's computed value.
fn read_as_flag(before: &[(&str, Option<bool>)], switches: &[&str]) -> bool {
    before.iter().all(|&(word, printed)| match printed {
        None => !matches!(word, "--" | "--end-of-options"),
        Some(text) => text,
    }) && before.last().is_none_or(|&(word, printed)| match printed {
        Some(_) => true,
        None => {
            !word.starts_with('-')
                || (word.starts_with("--") && word.contains('='))
                || switches.contains(&word)
        }
    })
}

/// A program's name when the word runs the system's own: the bare name, or
/// the name under `/bin`, `/usr/bin` or `/usr/local/bin`.
fn system_name(word: &str) -> Option<&str> {
    let name = ["/usr/local/bin/", "/usr/bin/", "/bin/"]
        .iter()
        .find_map(|folder| word.strip_prefix(folder))
        .unwrap_or(word);
    (!name.is_empty() && !name.contains('/')).then_some(name)
}

/// What bash reads at `((` or `$((`.
enum Parenthesis {
    /// Arithmetic: how many characters from the second `(` through the
    /// closing `))`, and whether it `names` a variable. Bash reads a
    /// variable's value as arithmetic in turn, and a subscript there runs
    /// commands.
    Arithmetic { length: usize, names: bool },
    /// A subshell, or a substitution's commands, because the `)` that closes
    /// the second `(` is not followed at once by another.
    Commands,
}

/// At the second `(` of `((` or `$((`: what bash reads from there. `None`
/// when that cannot be told: a quote, an escape, a backquote or a
/// substitution inside, or no `)` at all.
fn arithmetic(chars: &Chars<'_>) -> Option<Parenthesis> {
    let mut ahead = chars.clone();
    let mut depth = 0_usize;
    let mut length = 0_usize;
    let mut names = false;
    loop {
        length += 1;
        match ahead.next()? {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(if ahead.next() == Some(')') {
                        Parenthesis::Arithmetic {
                            length: length + 1,
                            names,
                        }
                    } else {
                        Parenthesis::Commands
                    });
                }
            }
            '\'' | '"' | '\\' | '`' => return None,
            '$' if ahead.peek() == Some(&'(') => return None,
            name if name == '$' || name == '_' || name.is_ascii_alphabetic() => names = true,
            _ => {}
        }
    }
}

/// What `$(` opens.
enum Dollar {
    /// Arithmetic, passed over. It `names` a variable, whose value bash
    /// reads as arithmetic too.
    Arithmetic { names: bool },
    /// A substitution's commands, read to the `)` that closes it.
    Substitution(Vec<Token>),
}

fn dollar_parenthesis(chars: &mut Chars<'_>) -> Option<Dollar> {
    if chars.peek() == Some(&'(')
        && let Parenthesis::Arithmetic { length, names } = arithmetic(chars)?
    {
        chars.nth(length - 1);
        return Some(Dollar::Arithmetic { names });
    }
    lex(chars, true).map(Dollar::Substitution)
}

/// Whether `$` and then `character` expand a parameter: a name, a digit, or
/// one of bash's own, such as `$_`, the last word of the command before.
fn parameter(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, '_' | '@' | '*' | '#' | '?' | '$' | '!' | '-')
}

/// After `$`: whether `${...}` or `$[...]` reads a value as a name or as
/// arithmetic (`${!x}`, `${x@P}`, `${a[i]}`, `${x:i}`, `$[x]`), where a
/// subscript runs commands. A name alone, its length, or a default, a
/// pattern or a change of case after it reads no value that way.
fn evaluates_a_value(chars: &Chars<'_>) -> bool {
    let mut ahead = chars.clone();
    match ahead.next() {
        Some('[') => true,
        Some('{') => {
            // `${#}` is how many arguments there are; `${#x}` is a length.
            if ahead.next_if_eq(&'#').is_some() && ahead.peek() == Some(&'}') {
                return false;
            }
            match ahead.next() {
                Some(first) if first.is_ascii_alphabetic() || first == '_' => {
                    while ahead
                        .next_if(|next| next.is_ascii_alphanumeric() || *next == '_')
                        .is_some()
                    {}
                    // `${a[@]}`, `${a[*]}` and `${a[2]}` read no value as
                    // arithmetic; any other subscript may.
                    if ahead.next_if_eq(&'[').is_some() {
                        let all = ahead.next_if(|next| matches!(next, '@' | '*')).is_some();
                        let mut digits = false;
                        while !all && ahead.next_if(char::is_ascii_digit).is_some() {
                            digits = true;
                        }
                        if !(all || digits) || ahead.next_if_eq(&']').is_none() {
                            return true;
                        }
                    }
                }
                Some(first) if first.is_ascii_digit() => {
                    while ahead.next_if(char::is_ascii_digit).is_some() {}
                }
                Some('@' | '*' | '?' | '-' | '$' | '0') => {}
                _ => return true,
            }
            match ahead.next() {
                Some('}' | '-' | '=' | '?' | '+' | '#' | '%' | '/' | '^' | ',') => false,
                Some(':') => !matches!(ahead.peek(), Some('-' | '=' | '?' | '+')),
                _ => true,
            }
        }
        _ => false,
    }
}

/// After `${` or `$[`: whether what follows the parameter is a pattern or
/// a change of case (`#`, `%`, `/`, `^`, `,`), whose quotes bash keeps in
/// double quotes. There a default, an alternative, an assigned value and
/// an error message (`-`, `+`, `=`, `?`, with a `:` or without) are
/// double-quoted text, and so is any form not told apart here.
fn keeps_its_quotes(chars: &Chars<'_>) -> bool {
    let mut ahead = chars.clone();
    // A special parameter is one character; a name or a number runs on.
    if ahead
        .next_if(|next| matches!(next, '@' | '*' | '#' | '?' | '-' | '$' | '!'))
        .is_none()
    {
        while ahead
            .next_if(|next| next.is_ascii_alphanumeric() || *next == '_')
            .is_some()
        {}
    }
    if ahead.next_if_eq(&'[').is_some() {
        while ahead.next_if(|next| *next != ']').is_some() {}
        ahead.next();
    }
    matches!(ahead.next(), Some('#' | '%' | '/' | '^' | ','))
}

/// After a `$` in double quotes: whether bash expands it to the positional
/// parameters or an array's elements, each a word of its own (`"$@"`,
/// `"${@}"`, `"${a[@]}"`), not to their count. `${!...}` reads a value as
/// a name, which `evaluates_a_value` already holds unread.
fn quoted_words(chars: &Chars<'_>) -> bool {
    let mut ahead = chars.clone();
    match ahead.next() {
        Some('@') => true,
        Some('{') => {
            let inside: String = ahead.take_while(|next| *next != '}').collect();
            !inside.starts_with('#') && (inside.starts_with('@') || inside.contains("[@]"))
        }
        _ => false,
    }
}

/// The body of a `$'...'` string, whose opening has been read, as written,
/// to the `'` that ends it. Bash finds the end first, reading a backslash
/// and the character after it as a pair, so `\'` ends nothing. `None` when
/// nothing ends it.
fn ansi_c_body(chars: &mut Chars<'_>) -> Option<String> {
    let mut body = String::new();
    loop {
        match chars.next()? {
            '\'' => return Some(body),
            '\\' => {
                body.push('\\');
                body.push(chars.next()?);
            }
            other => body.push(other),
        }
    }
}

/// The text bash makes of a `$'...'` string's body: each escape read
/// (`ansi_c_escape`), up to a `\0`, which ends what it keeps (ADR 0190).
fn ansi_c_text(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut escapes = body.chars().peekable();
    while let Some(next) = escapes.next() {
        let text = if next == '\\' {
            ansi_c_escape(&mut escapes)
        } else {
            next.to_string()
        };
        if let Some((kept, _)) = text.split_once('\0') {
            out.push_str(kept);
            break;
        }
        out.push_str(&text);
    }
    out
}

/// One escape in a `$'...'` string's text, after its backslash: the text
/// bash 5.2 makes of it. `\cX` is the control letter of X, a backslash
/// after `\c` takes a second one with it, and an escape bash does not name
/// is kept as written.
fn ansi_c_escape(chars: &mut Chars<'_>) -> String {
    let Some(named) = chars.next() else {
        return "\\".to_owned();
    };
    let letter = match named {
        'a' => '\u{7}',
        'b' => '\u{8}',
        'e' | 'E' => '\u{1b}',
        'f' => '\u{c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'v' => '\u{b}',
        '\\' | '\'' | '"' | '?' => named,
        'c' => match chars.next() {
            None => return "\\c".to_owned(),
            Some('?') => '\u{7f}',
            Some('\\') => {
                chars.next_if_eq(&'\\');
                '\u{1c}'
            }
            Some(control) if control.is_ascii() => {
                u8::try_from(control).map_or(control, |byte| char::from(byte & 0x1f))
            }
            Some(other) => return format!("\\c{other}"),
        },
        'x' | 'u' | 'U' | '0'..='7' => {
            let (radix, most) = match named {
                'x' => (16, 2),
                'u' => (16, 4),
                'U' => (16, 8),
                _ => (8, 3),
            };
            let mut digits = if radix == 8 {
                named.to_string()
            } else {
                String::new()
            };
            while digits.len() < most
                && let Some(digit) = chars.next_if(|next| next.is_digit(radix))
            {
                digits.push(digit);
            }
            // Bash keeps the low byte of an octal escape past 255.
            let value = u32::from_str_radix(&digits, radix)
                .ok()
                .map(|value| if radix == 8 { value & 0xff } else { value });
            match value.and_then(char::from_u32) {
                Some(letter) => letter,
                None if radix == 8 => return format!("\\{digits}"),
                None => return format!("\\{named}{digits}"),
            }
        }
        other => return format!("\\{other}"),
    };
    letter.to_string()
}

/// At a `(`: whether it is `()` after a name, which defines a function.
/// bash reads no other `()`: an empty subshell does not parse.
fn defines_a_function(chars: &Chars<'_>) -> bool {
    let mut ahead = chars.clone();
    while ahead.next_if(|next| matches!(next, ' ' | '\t')).is_some() {}
    ahead.next() == Some(')')
}

/// Words after which bash reads a command, which may be a merge.
const RESERVED: &[&str] = &[
    "!", "{", "then", "do", "else", "elif", "if", "while", "until", "time",
];

fn take_backquoted(chars: &mut Chars<'_>) -> Option<String> {
    let mut inner = String::new();
    loop {
        match chars.next()? {
            '`' => return Some(inner),
            '\\' => inner.push(chars.next()?),
            other => inner.push(other),
        }
    }
}

/// The word after `<<` or `<<-`, as bash takes its quotes off, and whether
/// any part of it was quoted. `None` when there is no word, or when bash
/// would read it in a way this does not: `$'...'`, `$"..."`, a `$(`, `${`
/// or `$[`, or a backquote.
fn heredoc_delimiter(chars: &mut Chars<'_>) -> Option<(String, bool)> {
    let expands = |chars: &Chars<'_>| matches!(chars.clone().next(), Some('(' | '{' | '['));
    while matches!(chars.peek(), Some(' ' | '\t')) {
        chars.next();
    }
    let mut word = String::new();
    let mut quoted = false;
    while let Some(&next) = chars.peek() {
        if matches!(
            next,
            ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>'
        ) {
            break;
        }
        chars.next();
        match next {
            '\'' => {
                quoted = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        other => word.push(other),
                    }
                }
            }
            // In double quotes a backslash is kept unless it escapes `$`, a
            // backquote, `"`, a backslash or a newline.
            '"' => {
                quoted = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            '\n' => {}
                            escaped @ ('$' | '`' | '"' | '\\') => word.push(escaped),
                            other => {
                                word.push('\\');
                                word.push(other);
                            }
                        },
                        '`' => return None,
                        '$' if expands(chars) => return None,
                        other => word.push(other),
                    }
                }
            }
            // A backslash and newline join the lines, and quote nothing.
            '\\' => match chars.next()? {
                '\n' => {}
                escaped => {
                    quoted = true;
                    word.push(escaped);
                }
            },
            '`' => return None,
            '$' if matches!(chars.peek(), Some('\'' | '"')) || expands(chars) => return None,
            other => word.push(other),
        }
    }
    (quoted || !word.is_empty()).then_some((word, quoted))
}

/// The lines up to the one that is only the delimiter, which is read and
/// left out. In an unquoted body a backslash and newline join two lines, as
/// bash reads it, before the line is compared and in the body returned.
/// `None` when the text ends first, or, inside a `$(`, at a line that starts
/// with the delimiter and goes on, where bash 5.2 ends the body too.
fn heredoc_body(chars: &mut Chars<'_>, heredoc: &Heredoc, in_substitution: bool) -> Option<String> {
    let mut body = String::new();
    loop {
        chars.peek()?;
        let mut line = String::new();
        while let Some(character) = chars.next() {
            match character {
                '\n' => break,
                '\\' if !heredoc.quoted => match chars.next() {
                    Some('\n') => {}
                    Some(escaped) => {
                        line.push('\\');
                        line.push(escaped);
                    }
                    None => line.push('\\'),
                },
                other => line.push(other),
            }
        }
        let line = if heredoc.strip_tabs {
            line.trim_start_matches('\t')
        } else {
            &line
        };
        if line == heredoc.delimiter {
            return Some(body);
        }
        if in_substitution && line.starts_with(heredoc.delimiter.as_str()) {
            return None;
        }
        body.push_str(line);
        body.push('\n');
    }
}

/// The commands an unquoted heredoc's body runs as the shell reads it: its
/// `$(...)` and backquotes. Everything else in it is data.
fn expansions(body: &str) -> Option<Vec<Token>> {
    let mut found = Vec::new();
    let mut chars = body.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                chars.next();
            }
            '`' => {
                found.extend(tokens(&take_backquoted(&mut chars)?)?);
                found.push(Token::Break);
            }
            '$' if chars.next_if_eq(&'(').is_some() => match dollar_parenthesis(&mut chars)? {
                Dollar::Substitution(inner) => {
                    found.extend(inner);
                    found.push(Token::Break);
                }
                Dollar::Arithmetic { names: true } => found.push(Token::Opaque),
                Dollar::Arithmetic { names: false } => {}
            },
            '$' if evaluates_a_value(&chars) => found.push(Token::Opaque),
            _ => {}
        }
    }
    Some(found)
}

/// Every merge `command` makes, and the folder each runs in.
fn merges_in(command: &str, cwd: Option<&Path>, depth: usize) -> Parsed {
    let scope = Scope {
        depth,
        plain: true,
        names_hold: true,
        curl_configured: configures_curl(command),
        cdpath: if std::env::var_os("CDPATH").is_some_and(|path| !path.is_empty()) {
            Cdpath::Set
        } else {
            Cdpath::Unset
        },
    };
    // In a command that mentions a merge, a part bash computes or evaluates
    // where it decides what runs may be that merge (ADR 0190).
    match merges_within(command, cwd, scope) {
        (_, true) if mentions_a_merge(command) => Parsed::Unclear,
        (parsed, _) => parsed,
    }
}

/// What the text around the text being read, and the whole command, say
/// about reading it.
#[derive(Clone, Copy)]
struct Scope {
    depth: usize,
    /// Nothing around the text can run what a heredoc in it gives a command.
    plain: bool,
    /// No command around the text changes what a program's name runs.
    names_hold: bool,
    /// The whole command writes or names curl's own config, which can hold
    /// any request a later `curl` sends.
    curl_configured: bool,
    /// Whether `CDPATH` may be set where the text runs: in Swarm's own
    /// environment, or by the text around it, which a shell it runs
    /// inherits.
    cdpath: Cdpath,
}

/// Whether `cd` may look for a folder in the folders `CDPATH` names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cdpath {
    Unset,
    Set,
}

impl Scope {
    fn inner(self, plain: bool, names_hold: bool, cdpath: Cdpath) -> Self {
        Self {
            depth: self.depth + 1,
            plain,
            names_hold,
            cdpath,
            ..self
        }
    }
}

/// The merges in text a command runs or reads, and whether a part of what
/// runs is one bash computes or evaluates where it decides what runs.
fn merges_within(text: &str, cwd: Option<&Path>, scope: Scope) -> (Parsed, bool) {
    match tokens(text).filter(|_| scope.depth <= MAX_NESTING) {
        Some(tokens) => merges_among(&tokens, cwd, scope),
        None if mentions_a_merge(text) => (Parsed::Unclear, false),
        None => (Parsed::Merges(Vec::new()), false),
    }
}

/// The merges among one command's tokens, and whether a part of what runs
/// is one bash computes or evaluates where it decides what runs: a
/// program, a word gh or curl reads, a value read as arithmetic or a name,
/// or a function defined (ADR 0190).
///
/// A heredoc's body is read as commands, as a file written from it may be
/// run later. A body that cannot be read as commands is left as data only
/// when no command here, nor any around it, can run what it is given, with
/// what an unquoted one expands still read (ADR 0188). What bash computes
/// in such a body runs only as it expands.
fn merges_among(tokens: &[Token], cwd: Option<&Path>, scope: Scope) -> (Parsed, bool) {
    let data = scope.plain && runs_nothing_it_is_given(tokens);
    let names_hold = scope.names_hold && !redefines_names(tokens);
    let mut doubted = tokens.contains(&Token::Opaque) || defines_a_function_by_name(tokens);
    let mut folder = cwd.map(Path::to_path_buf);
    let searched = searches_cdpath(tokens, scope);
    // A word here spells a merge and gh or curl, as a string a variable
    // holds does.
    let spelled = tokens
        .iter()
        .filter_map(Token::said)
        .any(|(word, _)| may_merge(word));
    let mut merges = Vec::new();
    for pipeline in tokens.split(|token| *token == Token::Break) {
        let commands: Vec<Simple<'_>> = pipeline
            .split(|token| *token == Token::Pipe)
            .map(Simple::from)
            .collect();
        for (place, command) in commands.iter().enumerate() {
            let input = seen_input(&commands, place, names_hold);
            let (what, around) = step(&command.words, &command.printed, input.as_deref(), scope);
            doubted |= around.doubt;
            // Where this command runs: a wrapper may name another folder.
            let here = match &around.folder {
                Some(target) => moved(folder.clone(), Some(target)),
                None => folder.clone(),
            };
            let mut found = Vec::new();
            let mut read = around.runs;
            // A command that may not run, or that runs in a shell of its
            // own, as in a pipeline, leaves the folder untold if it moves
            // (ADR 0190).
            let elsewhere = command.apart || commands.len() > 1;
            match what {
                // A program of its own runs no `cd`: it moves nothing here.
                Step::ChangeFolder(_) | Step::LoseFolder if around.apart => {}
                Step::ChangeFolder(_) | Step::LoseFolder if elsewhere => folder = None,
                Step::ChangeFolder(Some(target))
                    if searched && !target.starts_with(['/', '.', '~']) =>
                {
                    folder = None;
                }
                Step::ChangeFolder(target) => folder = moved(folder, target.as_deref()),
                Step::LoseFolder => folder = None,
                Step::Merge(mut merge) => {
                    merge.folder.clone_from(&here);
                    found.push(merge);
                }
                Step::Nested(text) => read.push(text),
                // A program this does not know may run what it reads from a
                // pipe or a redirection, which the gate cannot see, as a
                // merge: one a word of the command spells, as a variable
                // may hold it, or one echo or printf before it in the pipe
                // prints (ADR 0190). A heredoc it reads is read as commands
                // below.
                Step::Unread => {
                    doubted |= (place > 0 || command.redirected)
                        && input.is_none()
                        && (spelled
                            || commands[..place]
                                .iter()
                                .any(|before| prints_a_merge(&before.words)));
                }
                Step::Other => {}
            }
            // Commands run in this shell, as `eval` and `source` run them,
            // may move it where this does not follow.
            if around.moves_here {
                folder = None;
            }
            let cdpath = if searched { Cdpath::Set } else { Cdpath::Unset };
            let inner = scope.inner(data, names_hold, cdpath);
            for &(body, expands) in &command.heredocs {
                let expanded = || {
                    expansions(body).map_or((Parsed::Unclear, false), |run| {
                        merges_among(&run, here.as_deref(), inner)
                    })
                };
                let (body_merges, body_doubted) = match merges_within(body, here.as_deref(), inner)
                {
                    (Parsed::Unclear, _) if data && expands => expanded(),
                    (Parsed::Unclear, _) if data => (Parsed::Merges(Vec::new()), false),
                    // A body that is data, with everything it expands,
                    // runs nothing it is given: what it would doubt as
                    // commands is only printed.
                    (body_merges, _) if data => (body_merges, false),
                    body => body,
                };
                doubted |= body_doubted;
                match body_merges {
                    Parsed::Merges(body_merges) => found.extend(body_merges),
                    Parsed::Unclear => return (Parsed::Unclear, doubted),
                }
            }
            for text in read {
                let (nested, nested_doubted) = merges_within(&text, here.as_deref(), inner);
                doubted |= nested_doubted;
                match nested {
                    Parsed::Merges(nested) => found.extend(nested),
                    Parsed::Unclear => return (Parsed::Unclear, doubted),
                }
            }
            // A wrapper that runs gh as another account or elsewhere leaves
            // every merge it runs unread.
            for mut merge in found {
                if merge.unreadable.is_none() {
                    merge.unreadable = around.unreadable;
                }
                merges.push(merge);
            }
        }
    }
    // A variable gh or curl reads to choose the repository, the account or
    // the program, set or cleared anywhere in the command, may decide what
    // any of its merges is.
    if sets_what_gh_reads(tokens) {
        for merge in &mut merges {
            merge.unreadable.get_or_insert(SETS_WHAT_GH_READS);
        }
    }
    (Parsed::Merges(merges), doubted)
}

const SETS_WHAT_GH_READS: &str = "it sets or clears a variable gh reads to choose the repository, the account or the program, so Swarm cannot ask gh what it would merge";

/// Whether a command mentions a merge, as written or as bash may spell it:
/// with its quotes, backslashes and empty substitutions taken out, with
/// each escape that names a letter by its number read and the rest kept,
/// or with every escape such as `\x6d`, `\t` or a JSON `g` read as its
/// letter, as printf reads them, and with an address's `%6d` read as its
/// letter once bash has taken its quotes out. The grant mod sends the gate
/// a command only when one of these also names gh or curl (ADR 0190).
fn mentions_a_merge(text: &str) -> bool {
    spellings(text).iter().any(|spelling| {
        let lower = spelling.to_ascii_lowercase();
        lower.contains("merge") || lower.contains("enqueuepullrequest")
    })
}

/// The text as written, without what bash takes out of a word, and that
/// with its `%HH` escapes read, as a server reads an address once bash has
/// made it. Then, with each escape that names a letter by its number read
/// (`numbered`): its quotes taken out and its `%HH` escapes read, and
/// without what bash expands, which may be empty. Last, with every escape
/// read (`unescaped`), as printf reads them, without what bash expands.
/// Bash makes a letter only from a letter, a backslash and a letter, or
/// such a number in `$'...'`, so these find each merge it spells, wherever
/// a `$'...'` string begins, and the last the tab or newline printf makes
/// before gh or curl (ADR 0190).
fn spellings(text: &str) -> [String; 6] {
    let bare = without_quotes(text);
    let numbered = numbered(text);
    [
        text.to_owned(),
        percent_decoded(&bare),
        bare,
        percent_decoded(&without_quotes(&numbered)),
        without_quotes(&without_expansions(&numbered)),
        without_quotes(&without_expansions(&unescaped(text))),
    ]
}

/// The text with each run of `%HH` escapes read as the text its bytes make,
/// as a server may read an address's path. A run that makes no UTF-8 is
/// kept as written, as the grant mod keeps it.
fn percent_decoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let run = &rest[start..];
        rest = run;
        let mut bytes = Vec::new();
        while let Some(byte) = rest
            .strip_prefix('%')
            .and_then(|after| after.get(..2))
            .filter(|hex| hex.bytes().all(|digit| digit.is_ascii_hexdigit()))
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            bytes.push(byte);
            rest = &rest[3..];
        }
        if bytes.is_empty() {
            out.push('%');
            rest = &rest[1..];
            continue;
        }
        match String::from_utf8(bytes) {
            Ok(decoded) => out.push_str(&decoded),
            Err(_) => out.push_str(&run[..run.len() - rest.len()]),
        }
    }
    out.push_str(rest);
    out
}

/// The text without a backslash and newline, a `$` before a quote, and
/// every quote and backslash.
fn without_quotes(text: &str) -> String {
    text.replace("\\\n", "")
        .replace("$'", "'")
        .replace("$\"", "\"")
        .chars()
        .filter(|character| !matches!(character, '\'' | '"' | '\\'))
        .collect()
}

/// The text without what bash expands in a word: `${...}`, a `$(...)` with
/// no parenthesis inside, a backquoted command, `$` and a name, and `$` and
/// a digit or one of bash's own parameters. The grant mod reads a command
/// the same way (ADR 0190).
fn without_expansions(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(&character) = chars.get(at) {
        let rest = &chars[at..];
        // The length of an expansion that opens and closes, with none of
        // `stops` before its close.
        let closed = |open: usize, close: char, stops: &[char]| {
            rest[open..]
                .iter()
                .position(|next| *next == close || stops.contains(next))
                .filter(|&found| rest[open + found] == close)
                .map(|found| open + found + 1)
        };
        let expansion = match rest {
            ['$', '{', ..] => closed(2, '}', &[]),
            ['$', '(', ..] => closed(2, ')', &['(']),
            ['`', ..] => closed(1, '`', &[]),
            ['$', first, ..] if first.is_ascii_alphabetic() || *first == '_' => Some(
                1 + rest[1..]
                    .iter()
                    .take_while(|next| next.is_ascii_alphanumeric() || **next == '_')
                    .count(),
            ),
            ['$', first, ..] if first.is_ascii_digit() || "@*#?$!-".contains(*first) => Some(2),
            _ => None,
        };
        if let Some(length) = expansion {
            at += length;
        } else {
            out.push(character);
            at += 1;
        }
    }
    out
}

/// The text with each escape printf or JSON reads as a letter read as it,
/// wherever it stands: `\xHH`, `\NNN` in octal, which keeps its low byte
/// as printf does, `\uHHHH`, `\UHHHHHHHH`, and the one-letter escapes, as
/// `\t` and `\n`. A request body's JSON reads `g` as `g`, so GitHub
/// does too.
fn unescaped(text: &str) -> String {
    escapes_read(text, true)
}

/// The text with each escape that names a letter by its number read as
/// that letter, wherever it stands, and each other escape kept, so a
/// backslash and a letter is that letter once its backslash is taken out,
/// as bash reads it outside `$'...'` (ADR 0190).
fn numbered(text: &str) -> String {
    escapes_read(text, false)
}

/// The text with each numbered escape read, and with each one-letter
/// escape too when `one_letter` says so.
fn escapes_read(text: &str, one_letter: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        let letter = match chars.peek() {
            _ if !one_letter => None,
            Some('a') => Some('\u{7}'),
            Some('b') => Some('\u{8}'),
            Some('e' | 'E') => Some('\u{1b}'),
            Some('f') => Some('\u{c}'),
            Some('n') => Some('\n'),
            Some('r') => Some('\r'),
            Some('t') => Some('\t'),
            Some('v') => Some('\u{b}'),
            _ => None,
        };
        if let Some(letter) = letter {
            chars.next();
            out.push(letter);
            continue;
        }
        let (radix, most) = match chars.peek() {
            Some('x') => (16, 2),
            Some('u') => (16, 4),
            Some('U') => (16, 8),
            Some('0'..='7') => (8, 3),
            _ => {
                out.push(character);
                continue;
            }
        };
        // The letter that names the escape, which `\NNN` has none of.
        let named = if radix == 16 { chars.next() } else { None };
        let mut digits = String::new();
        while digits.len() < most
            && let Some(digit) = chars.next_if(|next| next.is_digit(radix))
        {
            digits.push(digit);
        }
        if let Some(letter) = u32::from_str_radix(&digits, radix)
            .ok()
            .map(|value| if radix == 8 { value & 0xff } else { value })
            .and_then(char::from_u32)
        {
            out.push(letter);
        } else {
            out.push('\\');
            out.extend(named);
            out.push_str(&digits);
        }
    }
    out
}

/// Whether the command writes or names curl's own config, `.curlrc`, or
/// moves where curl looks for it, so a later `curl` may send anything.
fn configures_curl(text: &str) -> bool {
    spellings(text).iter().any(|spelling| {
        let lower = spelling.to_ascii_lowercase();
        lower.contains("curlrc") || lower.contains("curl_home")
    })
}

/// One simple command: its words, with what bash computes in each, each
/// heredoc it is given with whether that heredoc expands, the ones it reads
/// on standard input, and whether its standard input comes from a file, a
/// here-string, a descriptor, or a heredoc whose text bash changes.
struct Simple<'a> {
    words: Vec<String>,
    printed: Vec<Option<Printing>>,
    heredocs: Vec<(&'a str, bool)>,
    input: Vec<&'a str>,
    redirected: bool,
    /// It may not run, or runs in a shell of its own (`Token::Apart`).
    apart: bool,
}

impl<'a> From<&'a [Token]> for Simple<'a> {
    fn from(tokens: &'a [Token]) -> Self {
        let mut command = Simple {
            words: Vec::new(),
            printed: Vec::new(),
            heredocs: Vec::new(),
            input: Vec::new(),
            redirected: false,
            apart: false,
        };
        for token in tokens {
            match token {
                Token::Word(word) => {
                    command.words.push(word.clone());
                    command.printed.push(None);
                }
                Token::Printed {
                    word,
                    splits,
                    as_text,
                    leads,
                } => {
                    command.words.push(word.clone());
                    command.printed.push(Some(Printing {
                        splits: *splits,
                        as_text: *as_text,
                        leads: *leads,
                    }));
                }
                Token::Stdin {
                    body,
                    expands,
                    standard,
                } => {
                    command.heredocs.push((body, *expands));
                    // What bash makes of an unquoted body holding `$`, a
                    // backquote or a backslash is not its text (ADR 0190).
                    if *standard && *expands && body.contains(['$', '`', '\\']) {
                        command.redirected = true;
                    } else if *standard {
                        command.input.push(body);
                    }
                }
                Token::Input => command.redirected = true,
                Token::Apart => command.apart = true,
                Token::Opaque | Token::Break | Token::Pipe => {}
            }
        }
        command
    }
}

/// What a command reads on standard input, as far as the gate can see it:
/// its own heredocs on standard input, or those the commands before it pass
/// on down the pipe as written, a here-string bash computes no part of among
/// them. `None` is input it cannot see: a file, a here-string bash computes
/// in part or a descriptor, a command that prints anything else, or the
/// shell's own. A heredoc on another descriptor is none of these. Where a
/// name may run something other than its program (`names_hold` false), no
/// command is known to pass its input on.
fn seen_input(commands: &[Simple<'_>], place: usize, names_hold: bool) -> Option<Vec<String>> {
    let mut at = place;
    loop {
        let command = &commands[at];
        if command.redirected {
            return None;
        }
        if !command.input.is_empty() {
            return Some(
                command
                    .input
                    .iter()
                    .map(|body| (*body).to_owned())
                    .collect(),
            );
        }
        at = at.checked_sub(1)?;
        if !names_hold || !passes_on(&commands[at].words) {
            return None;
        }
    }
}

/// Whether `CDPATH` may be set where these commands run: around this text,
/// by a word that names it, or by a name bash computes, which may be it.
/// `cd` looks for a folder named without `/` or `.` first in each folder it
/// names, which the gate does not follow.
fn searches_cdpath(tokens: &[Token], scope: Scope) -> bool {
    scope.cdpath == Cdpath::Set
        || sets_a_name_bash_computes(tokens)
        || tokens
            .iter()
            .filter_map(Token::said)
            .any(|(word, _)| word.contains("CDPATH"))
}

/// Whether a command is echo or printf whose words, as they print them
/// together, spell a merge and gh or curl.
fn prints_a_merge(words: &[String]) -> bool {
    words.split_first().is_some_and(|(first, rest)| {
        matches!(system_name(first), Some("echo" | "printf")) && may_merge(&rest.join(" "))
    })
}

/// Whether a command prints what it reads, as it reads it: `cat` with no
/// file, or `tee`.
fn passes_on(words: &[String]) -> bool {
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    match system_name(first) {
        Some("cat") => rest.iter().all(|word| word == "-"),
        Some("tee") => true,
        _ => false,
    }
}

/// Whether a name in these commands may run something other than the
/// program it names: a function defined, an alias, a hashed path, a
/// builtin turned on or off, a script read into the shell, or `PATH` set,
/// by an assignment, by a command given its name, as `read` and `printf
/// -v` are, through a reference to it (`declare -n p=PATH`), or as a name
/// bash computes, which may be it.
fn redefines_names(tokens: &[Token]) -> bool {
    tokens.contains(&Token::Opaque)
        || defines_a_function_by_name(tokens)
        || sets_a_name_bash_computes(tokens)
        || tokens.iter().filter_map(Token::said).any(|(word, _)| {
            word == "PATH"
                || assignment(word).is_some_and(|(name, value)| name == "PATH" || value == "PATH")
        })
        || tokens
            .split(|token| matches!(token, Token::Break | Token::Pipe))
            .any(|command| {
                let simple = Simple::from(command);
                let (at, _) = program(&simple.words, &simple.printed);
                simple.words.get(at).is_some_and(|first| {
                    matches!(
                        system_name(first),
                        Some("alias" | "hash" | "enable" | "." | "source" | "shopt")
                    )
                })
            })
}

/// Whether a command defines a function with the `function` keyword, which
/// runs in place of any program of its name. One defined with `()` is
/// `Token::Opaque` already.
fn defines_a_function_by_name(tokens: &[Token]) -> bool {
    tokens
        .split(|token| matches!(token, Token::Break | Token::Pipe))
        .any(|command| {
            command
                .iter()
                .filter_map(Token::said)
                .find(|&(word, _)| !RESERVED.contains(&word))
                .is_some_and(|word| word == ("function", None))
        })
}

/// Whether a variable gh or curl reads to choose the repository, the
/// account or the program is set or cleared anywhere in these commands: an
/// assignment of one, as a command's prefix or `export`'s argument, its
/// name alone as a word, as `unset`, `read` and `env -u` take it, or a
/// name bash computes, which may be one.
fn sets_what_gh_reads(tokens: &[Token]) -> bool {
    sets_a_name_bash_computes(tokens)
        || tokens.iter().filter_map(Token::said).any(|(word, _)| {
            let name = assignment(word)
                .map_or_else(|| word.strip_prefix("-u").unwrap_or(word), |(name, _)| name);
            is_name(name) && chooses_what_gh_merges(name)
        })
}

/// Whether a command here sets a variable whose name bash computes, which
/// may be any name (ADR 0190): an argument of `declare`, `typeset`,
/// `local`, `export` or `readonly` that bash computes and that is no
/// assignment to a name as written, any it computes with `-n`, which makes
/// a reference, the name `printf -v` sets, or one `read`, `mapfile`,
/// `readarray` or `getopts` is given.
fn sets_a_name_bash_computes(tokens: &[Token]) -> bool {
    tokens
        .split(|token| matches!(token, Token::Break | Token::Pipe))
        .any(|command| {
            let simple = Simple::from(command);
            let (at, _) = program(&simple.words, &simple.printed);
            let Some(first) = simple.words.get(at) else {
                return false;
            };
            let arguments = &simple.words[at + 1..];
            let computed = &simple.printed[at + 1..];
            let mut given = arguments.iter().zip(computed);
            match system_name(first) {
                Some(name @ ("declare" | "typeset" | "local" | "export" | "readonly")) => {
                    let reference = !matches!(name, "export" | "readonly")
                        && arguments
                            .iter()
                            .any(|word| word.starts_with('-') && word.contains('n'));
                    given.any(|(word, printed)| {
                        printed.is_some() && (reference || assignment(word).is_none())
                    })
                }
                Some("printf") => match (arguments.first(), computed.first()) {
                    (Some(flag), Some(None)) if flag == "-v" => {
                        computed.get(1).is_some_and(Option::is_some)
                    }
                    (Some(flag), Some(printed)) => flag.starts_with("-v") && printed.is_some(),
                    _ => false,
                },
                Some("read" | "mapfile" | "readarray" | "getopts") => {
                    computed.iter().any(Option::is_some)
                }
                _ => false,
            }
        })
}

/// gh's own variables that change only how it prints or asks, not what it
/// merges, as whom or where.
const GH_HARMLESS: &[&str] = &[
    "GH_PROMPT_DISABLED",
    "GH_NO_UPDATE_NOTIFIER",
    "GH_NO_EXTENSION_UPDATE_NOTIFIER",
    "GH_SPINNER_DISABLED",
    "GH_FORCE_TTY",
    "GH_DEBUG",
    "GH_MDWIDTH",
    "GH_COLOR_LABELS",
    "GIT_TERMINAL_PROMPT",
];

/// A variable gh, git or curl reads to choose the repository, the account,
/// the host or the program itself: gh's own (`GH_REPO`, `GH_TOKEN`, ...)
/// other than those that only change how it prints or name a program to
/// open, git's, which decide the repository gh reads, and where each finds
/// its config.
fn chooses_what_gh_merges(name: &str) -> bool {
    ((name.starts_with("GH_") || name.starts_with("GIT_"))
        && !names_a_program(name)
        && !GH_HARMLESS.contains(&name))
        || name.starts_with("GITHUB_")
        || matches!(name, "HOME" | "PATH" | "XDG_CONFIG_HOME" | "CURL_HOME")
}

/// A variable whose value names a program another one runs: an editor, a
/// pager, a browser, a password helper or ssh for git, or a script bash
/// reads first.
fn names_a_program(name: &str) -> bool {
    ["EDITOR", "PAGER", "BROWSER", "ASKPASS", "SSH_COMMAND"]
        .iter()
        .any(|end| name.ends_with(end))
        || matches!(
            name,
            "GIT_SSH" | "GIT_EXTERNAL_DIFF" | "BASH_ENV" | "ENV" | "PROMPT_COMMAND" | "LESSOPEN"
        )
}

/// Whether no command here can run what it is given or a file it is named:
/// no value read as a name, and every command one of a few that print,
/// store or send what they read. That includes the commands a substitution
/// or a process substitution runs (ADR 0190), and those an unquoted heredoc
/// runs as it is read (ADR 0188).
fn runs_nothing_it_is_given(tokens: &[Token]) -> bool {
    let mut piped = false;
    !tokens.contains(&Token::Opaque)
        && tokens
            .split_inclusive(|token| matches!(token, Token::Break | Token::Pipe))
            .all(|command| {
                let fed = piped
                    || command
                        .iter()
                        .any(|token| matches!(token, Token::Stdin { .. } | Token::Input));
                piped = command.last() == Some(&Token::Pipe);
                inert(command, fed)
            })
        && tokens.iter().all(|token| match token {
            Token::Stdin {
                body,
                expands: true,
                ..
            } => expansions(body).is_some_and(|run| runs_nothing_it_is_given(&run)),
            _ => true,
        })
}

/// Programs that run nothing they read and no file they are named.
const INERT: &[&str] = &[
    "cat", "tee", "echo", "printf", "cd", "pwd", "true", "false", ":", "mkdir", "ls", "rm", "grep",
    "egrep", "fgrep",
];
/// gh's commands that send what they are given as text or as a request.
const GH_INERT: &[&str] = &[
    "pr", "issue", "release", "gist", "api", "label", "search", "repo",
];
/// git's commands that store, send or show what they are given.
const GIT_INERT: &[&str] = &[
    "add",
    "commit",
    "push",
    "status",
    "diff",
    "log",
    "show",
    "tag",
    "notes",
    "fetch",
    "rev-parse",
    "hash-object",
    "branch",
    "switch",
    "checkout",
];

/// One simple command that runs nothing it is given: a program named as
/// written, never through a wrapper, a path of its own or a word bash
/// computes. `fed` is whether it reads standard input from a heredoc, a
/// file or a pipe.
///
/// - `printf` counts without `-v`, which names a variable whose subscript
///   runs commands, and with a first word as written.
/// - gh and git count only with one of their own commands named as written,
///   and a word bash computes only as the value of a text flag
///   (`holds_text`), never splitting.
/// - gh or git fed counts only when it reads all of that input as text or
///   a request first, so an editor it opens reads none of it.
/// - gh counts without `--`, after which it hands words to git.
/// - gh counts without a flag that opens the editor or browser its config
///   names, which the same command may write (`gh_opens_a_program`), other
///   than as a text flag's value, and not as `gh gist edit`, which opens
///   the editor with no flag.
/// - git counts without a flag that names a program for it to run.
fn inert(command: &[Token], fed: bool) -> bool {
    let words: Vec<(&str, Option<bool>)> = command.iter().filter_map(Token::said).collect();
    let Some((&(first, None), arguments)) = words.split_first() else {
        return words.is_empty();
    };
    let text = arguments.iter().all(|&(_, printed)| printed != Some(false));
    match system_name(first) {
        Some("printf") => !matches!(
            arguments.first(),
            Some(&(word, printed)) if printed.is_some() || word.starts_with("-v")
        ),
        Some(name) if INERT.contains(&name) => true,
        Some("gh") => {
            let plain: Vec<String> = arguments.iter().map(|&(word, _)| word.to_owned()).collect();
            text && !arguments.contains(&("--", None))
                // A text flag's value is text, whatever it starts with.
                && !arguments.iter().enumerate().any(|(at, &(word, _))| {
                    gh_opens_a_program(word) && !holds_text(&words[..=at], word)
                })
                // It opens the editor with no flag.
                && !matches!(arguments, [("gist", None), ("edit", None), ..])
                && (!fed || gh_line(&plain).reads_all_input())
                && matches!(arguments.first(), Some(&(noun, None)) if GH_INERT.contains(&noun))
        }
        Some("git") => {
            text && !arguments
                .iter()
                .any(|&(word, _)| starts_a_long_flag(word, GIT_PROGRAM_FLAGS))
                && (!fed || git_reads_all_input(arguments))
                && git_command(arguments).is_some_and(|at| GIT_INERT.contains(&arguments[at].0))
        }
        _ => false,
    }
}

/// gh's one-letter flags that take a value in every gh command that has
/// them, read from `gh help reference` for gh 2.103.3. A command without
/// one refuses it and runs nothing. `-h` is not one: it is `--help`.
const GH_VALUE_LETTERS: &str = "ABDFHLORSTXbgjkoq";

/// Whether a gh flag may open the editor or the browser gh's config names:
/// `--editor` or `--web`, or a group of one-letter flags with an `e` or a
/// `w` among its flags. Whether a letter takes the rest of a group as its
/// value often depends on the command (`-f` is `--fill` to `gh pr create`),
/// so the group's flags end only at a letter that takes one in every
/// command, or at an `=`, after which pflag gives the letter before it the
/// rest (ADR 0188).
fn gh_opens_a_program(word: &str) -> bool {
    if let Some(long) = word.strip_prefix("--") {
        let name = long.split_once('=').map_or(long, |(name, _)| name);
        return matches!(name, "editor" | "web");
    }
    let Some(letters) = word.strip_prefix('-') else {
        return false;
    };
    for (at, letter) in letters.char_indices() {
        if matches!(letter, 'e' | 'w') {
            return true;
        }
        if GH_VALUE_LETTERS.contains(letter) || takes_the_rest(&letters[at + letter.len_utf8()..]) {
            return false;
        }
    }
    false
}

/// Whether pflag gives a one-letter flag the rest of its group, `rest`,
/// whether it takes a value or not: an `=` and then something.
fn takes_the_rest(rest: &str) -> bool {
    rest.len() > 1 && rest.starts_with('=')
}

/// git's long flags that name a program for it to run.
const GIT_PROGRAM_FLAGS: &[&str] = &["receive-pack", "exec", "upload-pack"];
/// git's long flags that read standard input before its message.
const GIT_FIRST_READERS: &[&str] = &["patch", "interactive"];

/// Whether `word` is a long flag git may read as one of these. git takes
/// any start of a long flag's name that only one of its flags begins with,
/// so every start counts.
fn starts_a_long_flag(word: &str, flags: &[&str]) -> bool {
    let Some(flag) = word.strip_prefix("--") else {
        return false;
    };
    let name = flag.split_once('=').map_or(flag, |(name, _)| name);
    !name.is_empty() && flags.iter().any(|long| long.starts_with(name))
}

/// Whether git reads all of its standard input as the message, `-F -`,
/// before its hooks or an editor run, so they read none of it. Its
/// `--patch` and `--interactive`, and a short `-p`, read it first.
fn git_reads_all_input(arguments: &[(&str, Option<bool>)]) -> bool {
    let message = arguments.contains(&("-F-", None))
        || arguments.contains(&("--file=-", None))
        || arguments
            .windows(2)
            .any(|pair| matches!(pair, [("-F" | "--file", None), ("-", None)]));
    message
        && !arguments.iter().any(|&(word, _)| {
            starts_a_long_flag(word, GIT_FIRST_READERS)
                || (!word.starts_with("--")
                    && word.len() > 1
                    && word.starts_with('-')
                    && word.contains('p'))
        })
}

/// Where git's command is among its arguments, after its `-C <folder>` and
/// `--no-pager`, when it is named as written.
fn git_command(arguments: &[(&str, Option<bool>)]) -> Option<usize> {
    let mut at = 0;
    loop {
        match &arguments[at..] {
            [("--no-pager", None), ..] => at += 1,
            [("-C", None), _, ..] => at += 2,
            [(_, None), ..] => return Some(at),
            _ => return None,
        }
    }
}

enum Step {
    /// `cd`, with its target; `None` is `cd` alone, which goes home.
    ChangeFolder(Option<String>),
    /// A move this does not follow: `popd`, `pushd` other than to one
    /// folder, or `cd` to a folder bash computes.
    LoseFolder,
    Merge(Merge),
    /// Text a shell runs as commands: `bash -c '...'`, `eval ...`.
    Nested(String),
    /// A program this does not know, with no command found among its
    /// words: it may run what it reads in its own way, as m4, tclsh and
    /// `make -f -` do.
    Unread,
    Other,
}

/// What the words around a command's program say about how it runs.
#[derive(Default)]
struct Around {
    /// The folder a wrapper runs it in: `env -C`.
    folder: Option<String>,
    /// Why a merge it makes cannot be checked: it runs as another account,
    /// through another program, or with what gh reads changed.
    unreadable: Option<&'static str>,
    /// It runs as a program of its own, so a `cd` in it moves nothing here.
    apart: bool,
    /// Text it runs as commands besides its program: `env -S`'s, or the
    /// value of a variable that names a program another one runs.
    runs: Vec<String>,
    /// What runs, or what decides what it does, is a word bash computes or
    /// a value it evaluates, which may be a merge (ADR 0190).
    doubt: bool,
    /// It runs commands in this shell that may move it: `eval` text with a
    /// `cd` in it, or a script `source` reads.
    moves_here: bool,
}

const THROUGH_ANOTHER_PROGRAM: &str =
    "it runs gh or curl through another program, which may run it as another account or elsewhere";
const ANOTHER_ENVIRONMENT: &str = "it runs gh with an environment Swarm cannot see";
const UNTOLD_FOLDER: &str = "it runs gh in a folder Swarm cannot tell";

/// What one simple command does, from its words, what bash computes in
/// each, and what it reads on standard input; `None` is input the gate
/// cannot see.
fn step(
    words: &[String],
    printed: &[Option<Printing>],
    input: Option<&[String]>,
    scope: Scope,
) -> (Step, Around) {
    let (at, mut around) = program(words, printed);
    let what = step_at(
        &words[at..],
        &printed[at..],
        input,
        scope,
        &mut around,
        true,
    );
    (what, around)
}

/// Programs that run the rest of their words as a command, as another
/// account, elsewhere or later. A word bash computes among those may be
/// the command they run.
const RUNNERS: &[&str] = &[
    "sudo", "doas", "runuser", "su", "watch", "flock", "ssh", "setsid", "chroot", "nsenter",
    "unshare", "parallel",
];

/// What a shell, `source`, `.` or `eval` runs, or `None` for any other
/// program. A shell runs the text after `-c`. A script it reads from a word
/// bash computes, as `<(...)` is, or from input the gate cannot see may be
/// any commands (ADR 0190), as may text after `-c`, or `eval`'s, that bash
/// computes in part: the shell parses what bash put there. One named as a
/// file is not read. `source` and `.` run a script in this shell, and
/// `eval` runs its words there.
fn script_step(
    name: &str,
    arguments: &[String],
    computed: &[Option<Printing>],
    input: Option<&[String]>,
    around: &mut Around,
) -> Option<Step> {
    let step = match name {
        _ if SHELLS.contains(&name) => match shell_script(arguments) {
            Script::Text(at) => {
                around.doubt |= computed[at].is_some();
                Step::Nested(arguments[at].clone())
            }
            Script::Operand(at) => {
                around.doubt |= computed[at].is_some();
                Step::Other
            }
            Script::Input => {
                around.doubt |= input.is_none();
                Step::Other
            }
        },
        "source" | "." => {
            around.doubt |= computed.first().is_some_and(Option::is_some);
            around.moves_here = true;
            Step::Other
        }
        "eval" => {
            around.doubt |= computed.iter().any(Option::is_some);
            let text = arguments.join(" ");
            around.moves_here = moves_the_shell(&text);
            Step::Nested(text)
        }
        _ => return None,
    };
    Some(step)
}

/// What the command whose program is `words[0]` does. `through` reads a
/// program this does not know as one that may run a command among its
/// words, as `sudo` and `ssh` do.
fn step_at(
    words: &[String],
    printed: &[Option<Printing>],
    input: Option<&[String]>,
    scope: Scope,
    around: &mut Around,
    through: bool,
) -> Step {
    let Some(first) = words.first() else {
        return Step::Other;
    };
    // A program bash computes may be any program (ADR 0190).
    if printed[0].is_some() {
        around.doubt = true;
        return Step::Other;
    }
    let arguments = &words[1..];
    let computed = &printed[1..];
    let name = first.rsplit('/').next().unwrap_or(first);
    // A variable that names a program another one runs. A value bash
    // computes in part may be any commands (ADR 0190).
    if matches!(
        name,
        "export" | "declare" | "typeset" | "local" | "readonly"
    ) {
        for (argument, printed) in arguments.iter().zip(computed) {
            if let Some((_, value)) = assignment(argument).filter(|(name, _)| names_a_program(name))
            {
                around.doubt |= printed.is_some();
                around.runs.push(value.to_owned());
            }
        }
    }
    if let Some(evaluated) = evaluates_what_it_is_given(name, arguments) {
        around.doubt |= evaluated;
        return Step::Other;
    }
    if let Some(step) = script_step(name, arguments, computed, input, around) {
        return step;
    }
    match name {
        "cd" => cd(arguments, computed),
        "pushd" => match (arguments, computed) {
            ([target], [None]) if !target.starts_with(['-', '+']) => {
                Step::ChangeFolder(Some(target.clone()))
            }
            _ => Step::LoseFolder,
        },
        "popd" => Step::LoseFolder,
        // `trap` runs its first word as commands when its signal comes.
        "trap" => arguments
            .iter()
            .position(|argument| !matches!(argument.as_str(), "-p" | "-l" | "--"))
            .map_or(Step::Other, |at| {
                around.doubt |= computed[at].is_some();
                Step::Nested(arguments[at].clone())
            }),
        "xargs" => xargs(arguments, computed, scope, around),
        // An alias, gh's or git's, runs its value as commands.
        "gh" => {
            around.doubt |= gh_doubts(arguments, computed);
            match gh_alias(arguments) {
                Some(text) => {
                    around.doubt |= computed.iter().any(Option::is_some);
                    Step::Nested(text)
                }
                None => gh(arguments, input, open_endpoint(arguments, computed))
                    .map_or(Step::Other, Step::Merge),
            }
        }
        "curl" => {
            let (merge, doubt) = curl(arguments, computed, input, scope.curl_configured);
            around.doubt |= doubt;
            merge.map_or(Step::Other, Step::Merge)
        }
        "git" => match git_runs(arguments) {
            Some(text) => {
                around.doubt |= computed.iter().any(Option::is_some);
                Step::Nested(text)
            }
            // git's other commands, as `rebase -x`, `bisect run` and
            // `submodule foreach`, may run a word they are given.
            None if through && !git_stores_or_shows(arguments, computed) => {
                run_through(name, arguments, computed, input, scope, around)
            }
            None => Step::Other,
        },
        "alias" => {
            around.doubt |= computed.iter().any(Option::is_some);
            Step::Nested(
                arguments
                    .iter()
                    .filter_map(|argument| argument.split_once('=').map(|(_, value)| value))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        _ if !through || INERT.contains(&name) || name == "export" => Step::Other,
        _ => {
            if interprets(name) {
                around.doubt |= program_doubts(arguments, computed, input);
            }
            run_through(name, arguments, computed, input, scope, around)
        }
    }
}

/// Whether git's command is one of those that store, send or show what
/// they are given (`GIT_INERT`), named as written.
fn git_stores_or_shows(arguments: &[String], computed: &[Option<Printing>]) -> bool {
    let said: Vec<(&str, Option<bool>)> = arguments
        .iter()
        .zip(computed)
        .map(|(word, printed)| {
            (
                word.as_str(),
                printed.map(|printed| !printed.splits && printed.as_text),
            )
        })
        .collect();
    git_command(&said).is_some_and(|at| GIT_INERT.contains(&said[at].0))
}

/// Where `cd` moves, as bash's does: past its flags, `-L`, `-P`, `-e` and
/// `-@` in a group or not, and a `--`, to its one folder, or home with
/// none. With more than one, or a flag it does not have, it refuses and
/// moves nowhere, which this does not follow (ADR 0190).
fn cd(arguments: &[String], computed: &[Option<Printing>]) -> Step {
    let flag = |at: usize| {
        computed[at].is_none()
            && arguments[at].strip_prefix('-').is_some_and(|letters| {
                !letters.is_empty() && letters.chars().all(|letter| "LPe@".contains(letter))
            })
    };
    let mut at = 0;
    while at < arguments.len() && flag(at) {
        at += 1;
    }
    let ended = arguments.get(at).is_some_and(|word| word == "--") && computed[at].is_none();
    at += usize::from(ended);
    match (&arguments[at..], &computed[at..]) {
        ([], _) => Step::ChangeFolder(None),
        ([target], [None]) if ended || !target.starts_with('-') => {
            Step::ChangeFolder(Some(target.clone()))
        }
        _ => Step::LoseFolder,
    }
}

/// What xargs runs: its command, given words from its input or a file,
/// which the gate does not see. Without `-I`, they follow its words, as
/// words bash computes that split; with it, each word holding its string
/// holds one. An option this does not know may be one that changes how it
/// reads, so it is not read (ADR 0190).
fn xargs(
    arguments: &[String],
    computed: &[Option<Printing>],
    scope: Scope,
    around: &mut Around,
) -> Step {
    let Some((start, replace)) = xargs_options(arguments, computed) else {
        around.doubt = true;
        return Step::Other;
    };
    // With no command it runs `echo`.
    if start == arguments.len() {
        return Step::Other;
    }
    let mut words = arguments[start..].to_vec();
    let mut printed = computed[start..].to_vec();
    let given = Printing {
        splits: replace.is_none(),
        as_text: false,
        leads: true,
    };
    if let Some(string) = &replace {
        for (word, printing) in words.iter().zip(printed.iter_mut()) {
            if word.contains(string.as_str()) {
                *printing = Some(given);
            }
        }
    } else {
        words.push(String::new());
        printed.push(Some(given));
    }
    // xargs reads standard input itself, for its command's words.
    step_at(&words, &printed, Some(&[]), scope, around, true)
}

/// Where xargs's command starts, past its options, and the string `-I`,
/// `-i` or `--replace` gives, which its input replaces. `None` when an
/// option is one this does not know, or a word bash computes.
fn xargs_options(
    arguments: &[String],
    computed: &[Option<Printing>],
) -> Option<(usize, Option<String>)> {
    const ALONE: &[&str] = &[
        "null",
        "no-run-if-empty",
        "verbose",
        "interactive",
        "exit",
        "open-tty",
        "show-limits",
        "help",
        "version",
    ];
    const VALUED: &[&str] = &[
        "arg-file",
        "delimiter",
        "max-args",
        "max-procs",
        "max-chars",
        "process-slot-var",
    ];
    let mut replace = None;
    let mut at = 0;
    while let Some(word) = arguments.get(at) {
        if !word.starts_with('-') || word == "-" {
            break;
        }
        if computed[at].is_some() {
            return None;
        }
        at += 1;
        if word == "--" {
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (flag, value) = long
                .split_once('=')
                .map_or((long, None), |(flag, value)| (flag, Some(value)));
            match flag {
                "replace" => replace = Some(value.unwrap_or("{}").to_owned()),
                "eof" | "max-lines" => {}
                _ if ALONE.contains(&flag) && value.is_none() => {}
                _ if VALUED.contains(&flag) => {
                    if value.is_none() {
                        computed.get(at)?;
                        at += 1;
                    }
                }
                _ => return None,
            }
            continue;
        }
        for (place, letter) in word.char_indices().skip(1) {
            let rest = &word[place + letter.len_utf8()..];
            match letter {
                '0' | 'r' | 't' | 'p' | 'x' | 'o' => continue,
                'e' | 'l' => {}
                'i' => replace = Some(if rest.is_empty() { "{}" } else { rest }.to_owned()),
                'a' | 'd' | 'E' | 'I' | 'L' | 'n' | 'P' | 's' => {
                    let value = if rest.is_empty() {
                        let value = arguments.get(at)?;
                        if computed[at].is_some() {
                            return None;
                        }
                        at += 1;
                        value.as_str()
                    } else {
                        rest
                    };
                    if letter == 'I' {
                        replace = Some(value.to_owned());
                    }
                }
                _ => return None,
            }
            break;
        }
    }
    Some((at, replace))
}

/// Shells, which run a script: the text after `-c`, a file, or what they
/// read on standard input.
const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash", "ksh", "mksh", "rbash", "ash"];

/// Where a shell's script comes from.
enum Script {
    /// The text after `-c`, the word at this place among its arguments.
    Text(usize),
    /// A file named by the word at this place among its arguments.
    Operand(usize),
    /// Standard input: no file named, or `-s`.
    Input,
}

/// Where a shell finds its script, from its arguments.
fn shell_script(arguments: &[String]) -> Script {
    let mut at = 0;
    let mut input = false;
    while let Some(word) = arguments.get(at) {
        if word == "--" || word == "-" {
            at += 1;
            break;
        }
        if word.starts_with("--") {
            at += 1 + usize::from(matches!(word.as_str(), "--rcfile" | "--init-file"));
            continue;
        }
        let Some(letters) = word
            .strip_prefix(['-', '+'])
            .filter(|letters| !letters.is_empty())
        else {
            break;
        };
        if word.starts_with('-') && letters.contains('c') {
            return if at + 1 < arguments.len() {
                Script::Text(at + 1)
            } else {
                Script::Input
            };
        }
        input |= letters.contains('s');
        // `-o` and `-O` take the next word, an option's name.
        at += 1 + usize::from(letters.ends_with(['o', 'O']));
    }
    if input || at >= arguments.len() {
        Script::Input
    } else {
        Script::Operand(at)
    }
}

/// Whether text run in this shell has a command that moves it.
fn moves_the_shell(text: &str) -> bool {
    tokens(text).is_none_or(|tokens| {
        tokens.iter().filter_map(Token::said).any(|(word, _)| {
            matches!(
                system_name(word),
                Some("cd" | "pushd" | "popd" | "source" | ".")
            )
        })
    })
}

/// Programs that run code in a language of their own, which the gate does
/// not read: Python, Perl, Ruby, Node, PHP and awk.
fn interprets(name: &str) -> bool {
    name.starts_with("python")
        || matches!(
            name,
            "perl" | "ruby" | "node" | "nodejs" | "php" | "awk" | "gawk" | "mawk" | "nawk"
        )
}

/// For a program that runs code in a language of its own: whether that
/// code may be a merge. Code given in a word, or in input the gate sees,
/// that spells a merge and gh or curl may run it, as may code from a word
/// bash computes, its first that is no flag, or, with none, from input the
/// gate cannot see. With `--version` or `--help` among its flags it prints
/// about itself and exits, reading no code (ADR 0190).
fn program_doubts(
    arguments: &[String],
    computed: &[Option<Printing>],
    input: Option<&[String]>,
) -> bool {
    if arguments
        .iter()
        .chain(input.unwrap_or_default())
        .any(|text| may_merge(text))
    {
        return true;
    }
    match arguments
        .iter()
        .position(|argument| !argument.starts_with('-'))
    {
        Some(at) => computed[at].is_some(),
        None => {
            input.is_none()
                && !arguments
                    .iter()
                    .any(|argument| matches!(argument.as_str(), "--version" | "--help"))
        }
    }
}

/// Whether text spells a merge and gh or curl, as the grant mod looks for
/// one before it asks the gate (ADR 0190). gh and curl count as words of
/// their own, but anywhere in text holding a `$'...'` string, which may make
/// the tab or newline before one that bash splits a word at.
fn may_merge(text: &str) -> bool {
    let anywhere = text.contains("$'");
    mentions_a_merge(text)
        && spellings(text).iter().any(|spelling| {
            ["gh", "curl"].iter().any(|name| {
                if anywhere {
                    spelling.contains(name)
                } else {
                    names_a_word(spelling, name)
                }
            })
        })
}

/// Whether `name` stands in text as a word of its own: neither a letter, a
/// digit nor `_` beside it.
fn names_a_word(text: &str, name: &str) -> bool {
    let part = |character: char| character.is_ascii_alphanumeric() || character == '_';
    text.match_indices(name).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(part)
            && !text[at + name.len()..].chars().next().is_some_and(part)
    })
}

/// For a command that reads a value it is given as arithmetic or as a
/// name, where a subscript runs commands: whether it may (ADR 0190).
/// `None` for any other command.
fn evaluates_what_it_is_given(name: &str, arguments: &[String]) -> Option<bool> {
    let bracketed = || arguments.iter().any(|argument| argument.contains('['));
    match name {
        // `-i` reads every value given as arithmetic.
        "declare" | "typeset" | "local" | "readonly" => Some(
            bracketed()
                || arguments
                    .iter()
                    .any(|argument| argument.starts_with('-') && argument.contains('i')),
        ),
        "let" => Some(true),
        "[[" => Some(arguments.iter().any(|argument| {
            matches!(
                argument.as_str(),
                "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" | "-v"
            )
        })),
        // A name with a subscript, which runs commands as it is read.
        // printf reads one only as the name `-v` sets: bash 5.2 takes its
        // other words as text or numbers.
        "printf" => Some(
            arguments
                .first()
                .is_some_and(|first| first.starts_with("-v"))
                && bracketed(),
        ),
        "read" | "unset" | "mapfile" | "readarray" | "getopts" | "wait" | "test" | "[" => {
            Some(bracketed())
        }
        _ => None,
    }
}

/// A program this does not know, which may run a command among its words,
/// as `sudo -u w gh ...`, `ssh host gh ...` and `find . -exec gh ...` do:
/// the first such command, which runs apart and cannot be checked. A
/// runner's text that bash computes in part may be any commands, and what
/// a command among its words as written leaves in doubt, as xargs's words
/// or a subcommand bash computes, is in doubt here too: it reads this
/// one's input. With no such command among its words, a word that spells
/// a merge and gh or curl may be one it runs in its own way, as sed's `e`
/// and vim's `!` run theirs, and so may what it reads (`Step::Unread`; ADR
/// 0190).
fn run_through(
    name: &str,
    arguments: &[String],
    computed: &[Option<Printing>],
    input: Option<&[String]>,
    scope: Scope,
    around: &mut Around,
) -> Step {
    around.unreadable.get_or_insert(THROUGH_ANOTHER_PROGRAM);
    around.apart = true;
    if let Some(text) = shell_text(name, arguments) {
        around.doubt |= computed.iter().any(Option::is_some);
        return Step::Nested(text);
    }
    let runner = RUNNERS.contains(&name);
    for start in 0..arguments.len() {
        if runner && computed[start].is_some() {
            around.doubt = true;
        }
        let mut inner = Around::default();
        let found = step_at(
            &arguments[start..],
            &computed[start..],
            input,
            scope,
            &mut inner,
            false,
        );
        if matches!(found, Step::Merge(_) | Step::Nested(_)) {
            around.doubt |= inner.doubt;
            return found;
        }
        around.doubt |= inner.doubt && computed[start].is_none();
    }
    around.doubt |= arguments.iter().any(|text| may_merge(text));
    Step::Unread
}

/// The text a runner hands a shell as commands (ADR 0190): `su`, `runuser`,
/// `flock` and `script` the value of `-c`, and `ssh` and `watch` their words
/// after the host or the options, joined as they join them. `watch -n`'s
/// value is a number, which is read as a program whose words are read.
fn shell_text(name: &str, arguments: &[String]) -> Option<String> {
    let (value_letters, joins) = match name {
        "su" => ("cgGsw", false),
        "runuser" => ("cgGsuw", false),
        "flock" => ("cwE", false),
        "script" => ("cEIOBTm", false),
        "ssh" => ("BbcDEeFIiJLlmOopQRSWw", true),
        "watch" => ("", true),
        _ => return None,
    };
    let mut words = arguments.iter();
    let mut joined = Vec::new();
    while let Some(word) = words.next() {
        if let Some(long) = word.strip_prefix("--") {
            let (flag, attached) = long
                .split_once('=')
                .map_or((long, None), |(flag, value)| (flag, Some(value)));
            if !joins && matches!(flag, "command" | "session-command") {
                return attached
                    .map(str::to_owned)
                    .or_else(|| words.next().cloned());
            }
            continue;
        }
        let Some(letters) = word.strip_prefix('-').filter(|letters| !letters.is_empty()) else {
            // A user or a lock file before `-c` is passed over.
            if joins {
                joined.push(word.as_str());
                joined.extend(words.by_ref().map(String::as_str));
                break;
            }
            continue;
        };
        if let Some(at) = letters.find(|letter| value_letters.contains(letter)) {
            let rest = &letters[at + 1..];
            let value = if rest.is_empty() {
                words.next().cloned()
            } else {
                Some(rest.to_owned())
            };
            if !joins && letters[at..].starts_with('c') {
                return value;
            }
        }
    }
    // ssh's first word is the host; watch runs all of them.
    let commands = if name == "ssh" {
        joined.get(1..).unwrap_or_default()
    } else {
        &joined[..]
    };
    (joins && !commands.is_empty()).then(|| commands.join(" "))
}

/// `-` or `/dev/stdin`, as a file a request body is read from.
fn is_standard_input(file: &str) -> bool {
    matches!(file, "-" | "/dev/stdin")
}

/// Where a command's program is among its words, past its assignments, the
/// reserved words before it, and the wrappers that run the rest as a
/// command, with what those say about how it runs. A word bash computes
/// is matched as written, so it is one of these only where bash runs it as
/// written.
fn program(words: &[String], printed: &[Option<Printing>]) -> (usize, Around) {
    let mut around = Around::default();
    let mut at = 0;
    while let Some(word) = words.get(at) {
        if let Some((name, value)) = assignment(word) {
            // A value bash computes in part may be any commands.
            if names_a_program(name) {
                around.doubt |= printed[at].is_some();
                around.runs.push(value.to_owned());
            }
            at += 1;
            continue;
        }
        // `a[i]=x` reads its subscript as arithmetic, which runs commands.
        if word
            .split_once('=')
            .and_then(|(name, _)| name.split_once('['))
            .is_some_and(|(name, _)| is_name(name))
        {
            around.doubt = true;
            at += 1;
            continue;
        }
        let rest = &words[at + 1..];
        let after = &printed[at + 1..];
        at += 1 + match word.as_str() {
            // Bash reads a command after each of these.
            "!" | "{" | "then" | "do" | "else" | "elif" | "if" | "while" | "until" => 0,
            // `time -p --`: either option may be left out.
            "time" => {
                let posix = usize::from(rest.first().is_some_and(|next| next == "-p"));
                posix + usize::from(rest.get(posix).is_some_and(|next| next == "--"))
            }
            "function" => 1,
            // A coprocess runs in a shell of its own, so a simple command
            // it runs moves nothing here. A compound one's commands follow
            // one another there, and are read as apart (`Token::Apart`).
            "coproc" => {
                let opens = |next: &String| next == "case" || OPENERS.contains(&next.as_str());
                let named = rest.get(1).is_some_and(opens);
                around.apart |= !named && !rest.first().is_some_and(opens);
                usize::from(named)
            }
            _ => match system_name(word) {
                // A multi-call binary runs the applet named after it.
                Some("busybox" | "toybox") => 0,
                Some("command" | "builtin") => options(rest, &[]),
                Some("exec") => options(rest, &["-a"]),
                Some("nohup") => {
                    around.apart = true;
                    usize::from(rest.first().is_some_and(|next| next == "--"))
                }
                Some("nice") => {
                    around.apart = true;
                    options(rest, &["-n", "--adjustment"])
                }
                Some("stdbuf") => {
                    around.apart = true;
                    options(rest, &["-i", "-o", "-e", "--input", "--output", "--error"])
                }
                Some("timeout") => {
                    around.apart = true;
                    let flags = options(rest, &["-s", "-k", "--signal", "--kill-after"]);
                    // Then its duration.
                    flags + usize::from(rest.len() > flags)
                }
                Some("env") => {
                    around.apart = true;
                    env_options(rest, after, &mut around)
                }
                _ => break,
            },
        };
    }
    (at.min(words.len()), around)
}

/// How many of `rest` are a wrapper's options: words that start with `-`,
/// with the next word too after one of `with_value`, through a `--`.
fn options(rest: &[String], with_value: &[&str]) -> usize {
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        if word == "--" {
            return at + 1;
        }
        if !word.starts_with('-') || word == "-" {
            break;
        }
        at += 1 + usize::from(with_value.contains(&word.as_str()));
    }
    at
}

/// How many of `rest` are `env`'s options, noting the folder, the split
/// string or the environment they give the command.
fn env_options(rest: &[String], printed: &[Option<Printing>], around: &mut Around) -> usize {
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        let value = || rest.get(at + 1).map(String::as_str);
        let (flag, attached) = match word.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ if word.len() > 2 && word.starts_with('-') && !word.starts_with("--") => {
                env_letters(&word[1..], around)
            }
            _ => (word.as_str(), None),
        };
        let separate = attached.is_none();
        let given = attached.or_else(value);
        match flag {
            "--" => return at + 1,
            "-i" | "--ignore-environment" | "-" => {
                around.unreadable.get_or_insert(ANOTHER_ENVIRONMENT);
            }
            "-u" | "--unset" => {
                if given.is_some_and(chooses_what_gh_merges) {
                    around.unreadable.get_or_insert(SETS_WHAT_GH_READS);
                }
            }
            "-C" | "--chdir" => {
                let computed = printed
                    .get(at + usize::from(separate))
                    .is_some_and(Option::is_some);
                match given {
                    Some(folder) if !computed => around.folder = Some(folder.to_owned()),
                    _ => {
                        around.unreadable.get_or_insert(UNTOLD_FOLDER);
                    }
                }
            }
            // The string is split into the command and its first words. One
            // bash computes in part may be any commands (ADR 0190).
            "-S" | "--split-string" => {
                around.doubt |= printed
                    .get(at..)
                    .unwrap_or_default()
                    .iter()
                    .any(Option::is_some);
                let after = at + 1 + usize::from(separate);
                let mut text = given.unwrap_or_default().to_owned();
                for word in rest.get(after..).unwrap_or_default() {
                    text.push(' ');
                    text.push_str(&quoted(word));
                }
                around.runs.push(text);
                return rest.len();
            }
            "-0" | "--null" | "-v" | "--debug" | "--list-signal-handling" => {}
            flag if flag.starts_with("--")
                && ["--ignore-signal", "--default-signal", "--block-signal"]
                    .iter()
                    .any(|signal| flag.starts_with(signal)) => {}
            flag if flag.starts_with('-') => {
                around.unreadable.get_or_insert(ANOTHER_ENVIRONMENT);
            }
            _ => return at,
        }
        at += 1 + usize::from(separate && matches!(flag, "-u" | "--unset" | "-C" | "--chdir"));
    }
    at
}

/// A group of `env`'s one-letter flags, after its `-`, as env reads it:
/// `-i`, `-0` and `-v` take no value, and the first of `-u`, `-C` and `-S`
/// takes the rest of the group, or the next word when nothing is left.
/// Returns that flag and what the group gives it, or a flag env does not
/// have as the whole group.
fn env_letters<'a>(letters: &'a str, around: &mut Around) -> (&'a str, Option<&'a str>) {
    for (at, letter) in letters.char_indices() {
        match letter {
            'i' => {
                around.unreadable.get_or_insert(ANOTHER_ENVIRONMENT);
            }
            '0' | 'v' => {}
            'u' | 'C' | 'S' => {
                let flag = match letter {
                    'u' => "-u",
                    'C' => "-C",
                    _ => "-S",
                };
                let rest = &letters[at + 1..];
                return (flag, (!rest.is_empty()).then_some(rest));
            }
            _ => return ("-?", None),
        }
    }
    ("-0", None)
}

/// A word as bash reads it again: in single quotes.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// A word bash reads as an assignment, `NAME=value` or `NAME+=value`: its
/// name and value.
fn assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = word.split_once('=')?;
    let name = name.strip_suffix('+').unwrap_or(name);
    is_name(name).then_some((name, value))
}

/// A variable's name.
fn is_name(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && word
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn moved(folder: Option<PathBuf>, target: Option<&str>) -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    match target {
        None | Some("~") => home(),
        Some("-") => None,
        Some(target) if target.starts_with("~/") => home().map(|home| home.join(&target[2..])),
        Some(target) if target.starts_with('/') => Some(PathBuf::from(target)),
        Some(target) => folder.map(|folder| folder.join(target)),
    }
}

/// gh's flags that take a value, for `pr merge` and `api` together. Where the
/// two give a letter different meanings, both take a value.
const GH_VALUE_FLAGS: &[&str] = &[
    "-R",
    "--repo",
    "-A",
    "--author-email",
    "-b",
    "--body",
    "-F",
    "--body-file",
    "--match-head-commit",
    "-t",
    "--subject",
    "-X",
    "--method",
    "-f",
    "--raw-field",
    "--field",
    "-H",
    "--header",
    "--hostname",
    "--input",
    "-q",
    "--jq",
    "--template",
    "--cache",
    "-p",
    "--preview",
];

/// What a gh command line says, flag by flag.
#[derive(Default)]
struct GhLine {
    positionals: Vec<String>,
    repo: Option<String>,
    method: Option<String>,
    fields: Vec<String>,
    head: Option<String>,
    disables_auto_merge: bool,
    /// `--input`: the file the request body is read from.
    input: Option<String>,
    /// `--body-file`: the file a pull request's or issue's text is read from.
    body_file: Option<String>,
    /// The files the request is read from, other than standard input: a
    /// typed field's `key=@file` (`-f` sends an `@` as it is), or `--input`.
    files: Vec<String>,
}

impl GhLine {
    /// Whether gh sends its standard input as the request: `--input -`, or
    /// a field's `@-`.
    fn sends_input(&self) -> bool {
        self.input.as_deref().is_some_and(is_standard_input)
            || self.fields.iter().any(|field| {
                field
                    .split_once('=')
                    .and_then(|(_, value)| value.strip_prefix('@'))
                    .is_some_and(is_standard_input)
            })
    }

    /// Whether gh reads all of its standard input as text or a request
    /// before it does anything else, so nothing it opens can read any of it:
    /// the request, `--body-file -`, or `-F -`.
    fn reads_all_input(&self) -> bool {
        self.sends_input()
            || self.body_file.as_deref().is_some_and(is_standard_input)
            || self.fields.iter().any(|field| is_standard_input(field))
    }
}

fn gh_line(arguments: &[String]) -> GhLine {
    let mut line = GhLine::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let (flag, attached) = if let Some((flag, value)) = word.split_once('=')
            && flag.starts_with("--")
        {
            (flag.to_owned(), Some(value.to_owned()))
        } else if word.starts_with("--") {
            (word.clone(), None)
        } else if let Some(letters) = word.strip_prefix('-').filter(|letters| !letters.is_empty()) {
            let Some((flag, value)) = one_letter_value(letters) else {
                continue;
            };
            (flag, value)
        } else {
            line.positionals.push(word.clone());
            continue;
        };
        if flag == "--disable-auto" {
            line.disables_auto_merge = true;
        }
        if !GH_VALUE_FLAGS.contains(&flag.as_str()) {
            continue;
        }
        let Some(value) = attached.or_else(|| words.next().cloned()) else {
            continue;
        };
        match flag.as_str() {
            "-R" | "--repo" => line.repo = Some(value),
            "-X" | "--method" => line.method = Some(value),
            "-f" | "--raw-field" => line.fields.push(value),
            "-F" | "--field" => {
                if let Some(file) = value
                    .split_once('=')
                    .and_then(|(_, value)| value.strip_prefix('@'))
                    .filter(|file| !is_standard_input(file))
                {
                    line.files.push(file.to_owned());
                }
                line.fields.push(value);
            }
            "--match-head-commit" => line.head = Some(value),
            "--input" => {
                if !is_standard_input(&value) {
                    line.files.push(value.clone());
                }
                line.input = Some(value);
            }
            "--body-file" => line.body_file = Some(value),
            _ => {}
        }
    }
    line
}

/// The first flag in a group of one-letter flags (`letters`, after its `-`)
/// that takes a value, as pflag reads the group: the letters before it take
/// none, and it takes the rest of the word, less one `=`, or the next word
/// when nothing is left (`None` here). A letter before it that takes no
/// value ends the group at an `=`, taking the rest as its own (ADR 0188).
fn one_letter_value(letters: &str) -> Option<(String, Option<String>)> {
    for (at, letter) in letters.char_indices() {
        let rest = &letters[at + letter.len_utf8()..];
        if GH_VALUE_FLAGS.contains(&format!("-{letter}").as_str()) {
            let value = rest
                .strip_prefix('=')
                .filter(|value| !value.is_empty())
                .unwrap_or(rest);
            return Some((
                format!("-{letter}"),
                (!value.is_empty()).then(|| value.to_owned()),
            ));
        }
        if takes_the_rest(rest) {
            return None;
        }
    }
    None
}

/// `input` is what the command reads on standard input, which gh sends as the
/// request when told to read it: `--input -`, or a field's `@-`. `None` is
/// input the gate cannot see. `open` says `gh api`'s endpoint may be any
/// endpoint (`open_endpoint`).
fn gh(arguments: &[String], input: Option<&[String]>, open: bool) -> Option<Merge> {
    let line = gh_line(arguments);
    let body = if line.sends_input() {
        input
    } else {
        Some(&[][..])
    };
    let positional = |index: usize| line.positionals.get(index).map(String::as_str);
    match (positional(0), positional(1)) {
        (Some("pr"), Some("merge")) if !line.disables_auto_merge => Some(Merge {
            folder: None,
            repo: line.repo.clone(),
            pull_request: line.positionals.get(2).cloned(),
            head: line.head.clone().map_or(Head::Unnamed, Head::Named),
            unreadable: None,
        }),
        (Some("api"), Some(endpoint)) => {
            let graphql = open || graphql_endpoint(endpoint);
            let request = || line.fields.iter().chain(body.unwrap_or_default());
            if graphql && request().any(|part| names_a_merge_mutation(part)) {
                return Some(graphql_merge(
                    request().any(|part| part.contains("expectedHeadOid")),
                ));
            }
            if graphql && (body.is_none() || !line.files.is_empty()) {
                return Some(unseen_request());
            }
            // The merge API takes only PUT.
            if !line
                .method
                .as_deref()
                .is_some_and(|method| method.eq_ignore_ascii_case("PUT"))
            {
                return None;
            }
            let head = match line
                .fields
                .iter()
                .find_map(|field| field.strip_prefix("sha="))
            {
                Some(sha) => Head::Named(sha.to_owned()),
                None if body
                    .unwrap_or_default()
                    .iter()
                    .any(|part| part.contains("\"sha\"")) =>
                {
                    Head::Hidden
                }
                None => Head::Unnamed,
            };
            if open {
                return Some(Merge {
                    folder: None,
                    repo: None,
                    pull_request: None,
                    head,
                    unreadable: Some(COMPUTED_ADDRESS),
                });
            }
            let (repo, number) = merge_endpoint(endpoint)?;
            Some(Merge {
                folder: None,
                repo,
                pull_request: Some(number),
                head,
                unreadable: None,
            })
        }
        _ => None,
    }
}

const COMPUTED_ADDRESS: &str =
    "its address is a word bash computes, which may name any pull request's merge";

/// Where gh's command words are among its arguments: the words `gh_line`
/// reads as positionals, as their places.
fn gh_positionals(arguments: &[String]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(word) = arguments.get(at) {
        let takes_next = if word.starts_with("--") {
            !word.contains('=') && GH_VALUE_FLAGS.contains(&word.as_str())
        } else if let Some(letters) = word.strip_prefix('-').filter(|letters| !letters.is_empty()) {
            matches!(one_letter_value(letters), Some((_, None)))
        } else {
            found.push(at);
            false
        };
        at += 1 + usize::from(takes_next);
    }
    found
}

/// Whether a word bash computes may decide what a gh command does: one
/// before its command is named, or in `pr merge` or `api` any but one word
/// that is a text flag's value. In `gh api`, one that is the endpoint as
/// one operand (`open_endpoint`), or the value of a flag that only shapes
/// what gh prints, decides nothing more (ADR 0190).
fn gh_doubts(arguments: &[String], computed: &[Option<Printing>]) -> bool {
    let positions = gh_positionals(arguments);
    let word = |index: usize| positions.get(index).map(|&at| arguments[at].as_str());
    let api = word(0) == Some("api") && positions.first().is_some_and(|&at| computed[at].is_none());
    let open = open_endpoint(arguments, computed);
    let decides = |at: usize, printed: Printing| {
        let endpoint = open && positions.get(1) == Some(&at);
        let shapes_output = api && !printed.splits && prints_only(arguments, at);
        !endpoint && !shapes_output
    };
    let named = positions.get(1).map_or(arguments.len(), |&at| at + 1);
    if computed[..named]
        .iter()
        .enumerate()
        .any(|(at, printed)| printed.is_some_and(|printed| decides(at, printed)))
    {
        return true;
    }
    matches!(
        (word(0), word(1)),
        (Some("pr"), Some("merge")) | (Some("api"), _)
    ) && computed.iter().enumerate().any(|(at, printed)| {
        printed.is_some_and(|printed| decides(at, printed) && (printed.splits || !printed.as_text))
    })
}

/// Whether `gh api`'s endpoint is a word bash computes, one word, and its
/// only operand: it may be any endpoint, which gh reads as such. Were it a
/// flag instead, gh would have no endpoint and send nothing, as every
/// other word is read as written.
fn open_endpoint(arguments: &[String], computed: &[Option<Printing>]) -> bool {
    match gh_positionals(arguments).as_slice() {
        [noun, endpoint] => {
            arguments[*noun] == "api"
                && computed[*noun].is_none()
                && computed[*endpoint].is_some_and(|printed| !printed.splits)
        }
        _ => false,
    }
}

/// `gh api`'s flags whose value only shapes what it prints or how it asks.
const GH_API_OUTPUT_FLAGS: &[&str] = &[
    "-q",
    "--jq",
    "-t",
    "--template",
    "--cache",
    "-H",
    "--header",
    "-p",
    "--preview",
];

/// Whether the word at `at` is the value of one of `GH_API_OUTPUT_FLAGS`,
/// given after it or after its long name and `=`.
fn prints_only(arguments: &[String], at: usize) -> bool {
    at.checked_sub(1)
        .is_some_and(|before| GH_API_OUTPUT_FLAGS.contains(&arguments[before].as_str()))
        || GH_API_OUTPUT_FLAGS.iter().any(|flag| {
            flag.starts_with("--")
                && arguments[at]
                    .strip_prefix(flag)
                    .is_some_and(|rest| rest.starts_with('='))
        })
}

/// `gh alias set NAME EXPANSION`: what its name then runs, which is gh's
/// own words unless the expansion is a shell's, with `--shell` or a `!`
/// first.
fn gh_alias(arguments: &[String]) -> Option<String> {
    let line = gh_line(arguments);
    let positionals: Vec<&str> = line.positionals.iter().map(String::as_str).collect();
    let ["alias", "set", _, expansion, ..] = positionals.as_slice() else {
        return None;
    };
    let shell = arguments
        .iter()
        .any(|argument| argument == "--shell" || argument == "-s");
    Some(match expansion.strip_prefix('!') {
        Some(commands) => commands.to_owned(),
        None if shell => (*expansion).to_owned(),
        None => format!("gh {expansion}"),
    })
}

/// What git runs that its own words name: a `-c` setting's value, which may
/// be an alias (`!` first) or a program for git to run, and a value `git
/// config` sets, which a later git runs.
fn git_runs(arguments: &[String]) -> Option<String> {
    let mut runs = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-c" => runs.extend(
                words
                    .next()
                    .and_then(|setting| setting.split_once('='))
                    .map(|(_, value)| value),
            ),
            "-C" | "--git-dir" | "--work-tree" | "--namespace" => {
                words.next();
            }
            "config" => {
                let values: Vec<&String> = words
                    .by_ref()
                    .filter(|word| !word.starts_with('-'))
                    .collect();
                if let [_, value, ..] = values.as_slice() {
                    runs.push(value.as_str());
                }
                break;
            }
            flag if flag.starts_with('-') => {}
            _ => break,
        }
    }
    let commands: Vec<&str> = runs
        .iter()
        .map(|value| value.strip_prefix('!').unwrap_or(value))
        .collect();
    (!commands.is_empty()).then(|| commands.join("\n"))
}

/// curl's flags whose value, as one word bash computes, decides nothing
/// about what it sends where: a header, an account, a user agent, an
/// output file and what it prints.
const CURL_QUIET_FLAGS: &[&str] = &[
    "-H",
    "--header",
    "-u",
    "--user",
    "-A",
    "--user-agent",
    "-o",
    "--output",
    "-w",
    "--write-out",
];

/// curl's one-letter flags that take a value, from curl 8.5's manual. Every
/// other letter takes none, so a group of letters ends at one of these,
/// which takes the rest of the word or the next word.
const CURL_VALUE_LETTERS: &str = "AbcCdDeEFHKmoPQrtTuUwxXyYz";

/// curl's long flags that take the next word as their value, among those
/// this reads or whose value may look like an address.
const CURL_LONG_VALUE_FLAGS: &[&str] = &[
    "--request",
    "--data",
    "--data-ascii",
    "--data-binary",
    "--data-raw",
    "--data-urlencode",
    "--json",
    "--form",
    "--form-string",
    "--upload-file",
    "--config",
    "--variable",
    "--url",
    "--header",
    "--user",
    "--user-agent",
    "--output",
    "--write-out",
    "--cookie",
    "--cookie-jar",
    "--referer",
    "--proxy",
    "--cacert",
    "--cert",
    "--key",
    "--max-time",
    "--connect-timeout",
    "--retry",
    "--oauth2-bearer",
];

/// curl's long flags that decide what it sends or where. curl takes any
/// start of a long flag's name that only one of its flags begins with, so
/// a start of one of these that is not a flag of its own may be it.
const CURL_REQUEST_FLAGS: &[&str] = &[
    "--request",
    "--data",
    "--data-ascii",
    "--data-binary",
    "--data-raw",
    "--data-urlencode",
    "--json",
    "--form",
    "--form-string",
    "--upload-file",
    "--config",
    "--variable",
    "--url",
    "--globoff",
    "--disable",
];

/// What of a curl request the gate cannot see, the least first.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum Hidden {
    #[default]
    Nothing,
    /// Data read from a file, or from input the gate cannot see.
    Body,
    /// All of it: a config file, which can hold the address and the data,
    /// a value curl fills in from a variable (`--expand-*`), or a flag this
    /// cannot read.
    Request,
}

/// What a curl command line says it sends, and where.
#[derive(Default)]
struct CurlRequest {
    method: Option<String>,
    data: Vec<String>,
    urls: Vec<String>,
    /// An address is a word bash computes, which may be any address.
    any_url: bool,
    /// What of the request the gate cannot see.
    hidden: Hidden,
    /// curl reads `{...}` and `[...]` in an address as a pattern, unless
    /// told not to with `-g`.
    globbed: bool,
}

impl CurlRequest {
    /// One flag's value.
    fn read(&mut self, flag: &str, value: &str, input: Option<&[String]>) {
        match flag {
            "-X" | "--request" => self.method = Some(value.to_owned()),
            "-d" | "--data" | "--data-ascii" | "--data-binary" | "--json" => {
                self.from(value.strip_prefix('@'), value, input);
            }
            "-F" | "--form" if value.contains("=@") || value.contains("=<") => {
                self.hide(Hidden::Body);
            }
            "--data-raw" | "--form-string" | "-F" | "--form" => self.data.push(value.to_owned()),
            // `@file` or `name@file` reads a file; an `=` first is content.
            "--data-urlencode" => {
                let file = match (value.find('='), value.find('@')) {
                    (Some(equals), Some(at)) if at < equals => Some(&value[at + 1..]),
                    (None, Some(at)) => Some(&value[at + 1..]),
                    _ => None,
                };
                self.from(file, value, input);
            }
            // curl sends the file with PUT, unless `-X` names another
            // method, before or after.
            "-T" | "--upload-file" => {
                self.method.get_or_insert_with(|| "PUT".to_owned());
                self.from(Some(value), value, input);
            }
            "-K" | "--config" => self.hide(Hidden::Request),
            "--url" => self.urls.push(value.to_owned()),
            _ => {}
        }
    }

    /// Data given as it is, or read from `file`: standard input, seen or
    /// not, or a file the gate does not read.
    fn from(&mut self, file: Option<&str>, value: &str, input: Option<&[String]>) {
        match (file, input) {
            (Some(file), Some(input)) if is_standard_input(file) => {
                self.data.extend_from_slice(input);
            }
            (Some(_), _) => self.hide(Hidden::Body),
            (None, _) => self.data.push(value.to_owned()),
        }
    }

    /// Marks `hidden` unseen, and anything less.
    fn hide(&mut self, hidden: Hidden) {
        self.hidden = self.hidden.max(hidden);
    }

    /// Whether curl reads an address as a pattern it expands.
    fn pattern(&self, url: &str) -> bool {
        self.globbed && url.contains(['{', '['])
    }

    /// The merge the request makes, if it may make one. `reads_config`
    /// says curl reads a config of its own the command writes or names.
    fn merge(&self, reads_config: bool) -> Option<Merge> {
        if self.data.iter().any(|body| names_a_merge_mutation(body)) {
            return Some(graphql_merge(
                self.data
                    .iter()
                    .any(|body| body.contains("expectedHeadOid")),
            ));
        }
        let may_be_graphql = |url: &String| graphql_endpoint(url) || self.pattern(url);
        if self.hidden == Hidden::Request
            || reads_config
            || (self.hidden == Hidden::Body
                && (self.urls.is_empty() || self.any_url || self.urls.iter().any(may_be_graphql)))
        {
            return Some(unseen_request());
        }
        if !self
            .method
            .as_deref()
            .is_some_and(|method| method.eq_ignore_ascii_case("PUT"))
        {
            return None;
        }
        let head = if self.data.iter().any(|body| body.contains("\"sha\"")) {
            Head::Hidden
        } else {
            Head::Unnamed
        };
        if let Some((repo, number)) = self.urls.iter().find_map(|url| merge_endpoint(url)) {
            return Some(Merge {
                folder: None,
                repo,
                pull_request: Some(number),
                head,
                unreadable: None,
            });
        }
        let unreadable = if self.any_url {
            COMPUTED_ADDRESS
        } else if self.urls.iter().any(|url| self.pattern(url)) {
            "its address is a pattern curl expands, which may name any merge"
        } else {
            return None;
        };
        Some(Merge {
            folder: None,
            repo: None,
            pull_request: None,
            head,
            unreadable: Some(unreadable),
        })
    }
}

/// `input` is what the command reads on standard input, which curl sends as
/// data given as `@-`. `None` is input the gate cannot see.
/// `configured_here` says the command writes or names curl's own config,
/// which a `curl` not told to skip it (`-q` first) reads. Also says whether
/// a word bash computes may decide what curl sends where: any but one that
/// is an address as one operand (`Printing::one_operand`), or one word that
/// is the value of a flag in `CURL_QUIET_FLAGS` (ADR 0190).
fn curl(
    arguments: &[String],
    computed: &[Option<Printing>],
    input: Option<&[String]>,
    configured_here: bool,
) -> (Option<Merge>, bool) {
    let mut request = CurlRequest {
        globbed: true,
        ..CurlRequest::default()
    };
    let mut doubt = false;
    // `-q` first: curl reads no config of its own.
    let mut skips_config = false;
    let mut at = 0;
    // A flag's value, the word at `place`, read as written, and whether
    // bash computing it may decide what curl sends where.
    let value = |request: &mut CurlRequest, flag: &str, place: usize, text: &str| {
        request.read(flag, text, input);
        match computed[place] {
            None => false,
            // `--url` takes the next word as it is, whatever it begins with.
            Some(printed) if flag == "--url" && !printed.splits => {
                request.any_url = true;
                false
            }
            Some(printed) => printed.splits || !CURL_QUIET_FLAGS.contains(&flag),
        }
    };
    // An address, the word at `place`, and whether bash computing it may
    // decide more than the address.
    let address = |request: &mut CurlRequest, place: usize| {
        request.urls.push(arguments[place].clone());
        match computed[place] {
            None => false,
            Some(printed) if printed.one_operand(&arguments[place]) => {
                request.any_url = true;
                false
            }
            Some(_) => true,
        }
    };
    while let Some(word) = arguments.get(at) {
        let starts = at == 0;
        at += 1;
        // A flag bash computes may be any flag, `--` written before it too.
        if word.starts_with('-') && computed[at - 1].is_some() {
            doubt = true;
            continue;
        }
        if word == "--" {
            for place in at..arguments.len() {
                doubt |= address(&mut request, place);
            }
            break;
        }
        if word.starts_with("--") {
            match word.as_str() {
                "--globoff" => request.globbed = false,
                "--disable" if starts => skips_config = true,
                flag if flag.starts_with("--expand-") => {
                    at += 1;
                    request.hide(Hidden::Request);
                }
                flag if CURL_LONG_VALUE_FLAGS.contains(&flag) => {
                    if let Some(text) = arguments.get(at) {
                        doubt |= value(&mut request, flag, at, text);
                        at += 1;
                    }
                }
                // The start of a flag that decides the request, which curl
                // may take as that flag.
                flag if flag.len() > 2
                    && CURL_REQUEST_FLAGS
                        .iter()
                        .any(|known| known.len() > flag.len() && known.starts_with(flag)) =>
                {
                    request.hide(Hidden::Request);
                }
                _ => {}
            }
            continue;
        }
        let Some(letters) = word.strip_prefix('-').filter(|letters| !letters.is_empty()) else {
            doubt |= address(&mut request, at - 1);
            continue;
        };
        for (index, letter) in letters.char_indices() {
            if CURL_VALUE_LETTERS.contains(letter) {
                let rest = &letters[index + letter.len_utf8()..];
                let flag = format!("-{letter}");
                if rest.is_empty() {
                    if let Some(text) = arguments.get(at) {
                        doubt |= value(&mut request, &flag, at, text);
                        at += 1;
                    }
                } else {
                    doubt |= value(&mut request, &flag, at - 1, rest);
                }
                break;
            }
            match letter {
                'g' => request.globbed = false,
                'q' if starts && index == 0 => skips_config = true,
                _ => {}
            }
        }
    }
    (request.merge(configured_here && !skips_config), doubt)
}

/// Whether text names a GraphQL mutation that merges, as written or with
/// its JSON escapes read: GitHub reads `g` in a request body as `g`.
fn names_a_merge_mutation(text: &str) -> bool {
    let read = unescaped(text);
    [
        "mergePullRequest",
        "enablePullRequestAutoMerge",
        "enqueuePullRequest",
    ]
    .iter()
    .any(|mutation| text.contains(mutation) || read.contains(mutation))
}

/// A GraphQL request the gate cannot read, which may be any merge (ADR 0188).
fn unseen_request() -> Merge {
    Merge {
        folder: None,
        repo: None,
        pull_request: None,
        head: Head::Unnamed,
        unreadable: Some(
            "it reads its GraphQL request from a file or an input Swarm cannot see, so give the request in the command itself",
        ),
    }
}

/// Whether an endpoint or URL is GitHub's GraphQL endpoint: `graphql`, with
/// a leading `/`, or a URL whose path ends there, as gh reads all three. It
/// is read in any case of its letters, since whether GitHub does was not run.
fn graphql_endpoint(url: &str) -> bool {
    url.split(['?', '#']).next().is_some_and(|path| {
        let path = percent_decoded(path)
            .trim_end_matches('/')
            .to_ascii_lowercase();
        path == "graphql" || path.ends_with("/graphql")
    })
}

fn graphql_merge(head_named: bool) -> Merge {
    Merge {
        folder: None,
        repo: None,
        pull_request: None,
        head: if head_named {
            Head::Hidden
        } else {
            Head::Unnamed
        },
        unreadable: Some("it names the pull request by a GraphQL id"),
    }
}

/// `repos/<owner>/<name>/pulls/<number>/merge`, as a path or a URL, with
/// its `%HH` escapes read (ADR 0190). The repository is `None` when gh
/// fills it in from the folder (`{owner}`).
fn merge_endpoint(text: &str) -> Option<(Option<String>, String)> {
    let path = percent_decoded(text.split(['?', '#']).next()?);
    let path = &path[path.find("repos/")? + "repos/".len()..];
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    let [owner, name, "pulls", number, "merge"] = parts.as_slice() else {
        return None;
    };
    if number.is_empty() || !number.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    let repo =
        (!owner.starts_with('{') && !name.starts_with('{')).then(|| format!("{owner}/{name}"));
    Some((repo, (*number).to_owned()))
}

// ── Asking GitHub ───────────────────────────────────────────────────────────

/// The gh the worker itself runs, found the way its shell finds it.
pub(crate) struct GhCli {
    program: PathBuf,
}

impl Default for GhCli {
    fn default() -> Self {
        Self {
            program: PathBuf::from("gh"),
        }
    }
}

impl GhCli {
    /// gh as the worker's shell would run it, but never in colour.
    ///
    /// ⚠️ EVERY WORKER SESSION FORCES COLOUR. Swarm starts each terminal with
    /// `CLICOLOR_FORCE=1` and `FORCE_COLOR=3` and without `NO_COLOR`, to match
    /// the browser's xterm, and the gate inherits that environment. gh then
    /// colours its JSON even into a pipe, and `NO_COLOR` does not override
    /// `CLICOLOR_FORCE`; `GH_FORCE_TTY` colours it too unless `NO_COLOR` is
    /// set. `FORCE_COLOR` does not move gh 2.103.3 and goes as well, as the
    /// API's own gh calls already do. Only gh's environment changes here,
    /// never the session's.
    fn command(&self, folder: &Path, arguments: &[&str]) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.program);
        command
            .args(arguments)
            .current_dir(folder)
            .env_remove("CLICOLOR_FORCE")
            .env_remove("GH_FORCE_TTY")
            .env_remove("FORCE_COLOR")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    }

    async fn json(&self, folder: &Path, arguments: &[&str]) -> Result<Value, String> {
        let mut child = self
            .command(folder, arguments)
            .spawn()
            .map_err(|error| format!("gh could not be run: {error}"))?;
        let mut stdout = child.stdout.take().ok_or("gh gave no output")?;
        let answered = tokio::time::timeout(GH_TIMEOUT, async {
            let mut output = Vec::new();
            (&mut stdout)
                .take(MAX_GH_OUTPUT_BYTES)
                .read_to_end(&mut output)
                .await
                .map_err(|error| format!("gh's output could not be read: {error}"))?;
            let status = child
                .wait()
                .await
                .map_err(|error| format!("gh did not finish: {error}"))?;
            Ok::<_, String>((status, output))
        })
        .await
        .map_err(|_| format!("gh did not answer within {} seconds", GH_TIMEOUT.as_secs()))??;
        let (status, output) = answered;
        if !status.success() {
            return Err(format!("`gh {}` failed", arguments.join(" ")));
        }
        serde_json::from_slice(&output).map_err(|_| "gh's answer was not JSON".into())
    }
}

/// Who would make this merge, and who opened what it merges where.
async fn facts(gh: &GhCli, merge: &Merge) -> Result<Facts, String> {
    if let Some(why) = merge.unreadable {
        return Err(why.to_owned());
    }
    let folder = match (&merge.folder, &merge.repo) {
        (Some(folder), _) => folder.clone(),
        // A repository named outright does not depend on the folder.
        (None, Some(_)) => std::env::temp_dir(),
        (None, None) => return Err("Swarm cannot tell which folder the merge runs in".into()),
    };
    let text = |value: &Value, pointer: &str| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let user = gh.json(&folder, &["api", "user"]).await?;
    let merger = text(&user, "/login").ok_or("gh named no signed-in account")?;

    let mut view = vec!["pr", "view"];
    if let Some(pull_request) = &merge.pull_request {
        view.push(pull_request);
    }
    if let Some(repo) = &merge.repo {
        view.extend(["--repo", repo]);
    }
    view.extend(["--json", "author,headRefOid,number,url"]);
    let pull = gh.json(&folder, &view).await?;
    let author = text(&pull, "/author/login").ok_or("gh named no author")?;
    let head = text(&pull, "/headRefOid").ok_or("gh named no head commit")?;
    let number = pull
        .get("number")
        .and_then(Value::as_u64)
        .ok_or("gh named no number")?;
    let url = text(&pull, "/url").ok_or("gh named no address")?;
    let repo = url
        .split("github.com/")
        .nth(1)
        .and_then(|path| {
            let mut parts = path.split('/');
            Some(format!("{}/{}", parts.next()?, parts.next()?))
        })
        .ok_or("its address names no repository")?;

    let repository = gh
        .json(
            &folder,
            &["repo", "view", &repo, "--json", "owner,visibility"],
        )
        .await?;
    Ok(Facts {
        merger,
        repo_owner: text(&repository, "/owner/login")
            .ok_or("gh named no owner for the repository")?,
        public: text(&repository, "/visibility")
            .ok_or("gh did not say whether the repository is public")?
            .eq_ignore_ascii_case("public"),
        number,
        author_is_bot: pull
            .pointer("/author/is_bot")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || author.starts_with("app/")
            || author.ends_with("[bot]"),
        author,
        head,
        repo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn words(text: &str) -> Vec<String> {
        tokens(text)
            .unwrap()
            .into_iter()
            // Where a command may not run is read by `merges_in`'s tests.
            .filter(|token| *token != Token::Apart)
            .map(|token| match token {
                Token::Word(word) => word,
                Token::Printed {
                    word,
                    splits: false,
                    ..
                } => format!("<printed:{word}>"),
                Token::Printed {
                    word, splits: true, ..
                } => format!("<split:{word}>"),
                Token::Opaque => "<opaque>".to_owned(),
                Token::Break => "<break>".to_owned(),
                Token::Pipe => "<pipe>".to_owned(),
                Token::Input => "<input>".to_owned(),
                Token::Stdin { body, .. } => format!("<stdin:{body}>"),
                Token::Apart => unreachable!("filtered out above"),
            })
            .collect()
    }

    #[test]
    fn the_shell_is_read_as_the_shell_reads_it() {
        assert_eq!(
            words(r#"gh pr merge 5 --subject 'a; b' -b "it's \"x\"" 2>&1 >/dev/null # merge 9"#),
            [
                "gh",
                "pr",
                "merge",
                "5",
                "--subject",
                "a; b",
                "-b",
                "it's \"x\""
            ]
        );
        assert_eq!(
            words("cd /a && gh pr view 1\ngh pr merge 2|cat"),
            [
                "cd", "/a", "<break>", "gh", "pr", "view", "1", "<break>", "gh", "pr", "merge",
                "2", "<pipe>", "cat"
            ]
        );
        // A substitution's commands follow the command whose word holds what
        // it prints, so that command keeps all its words.
        assert_eq!(
            words(r#"echo "x $(gh pr merge 3) y" z"#),
            [
                "echo",
                "<printed:x  y>",
                "z",
                "<break>",
                "gh",
                "pr",
                "merge",
                "3",
                "<break>"
            ]
        );
        assert_eq!(
            words("a $(b)c | d; e"),
            [
                "a",
                "<split:c>",
                "<pipe>",
                "d",
                "<break>",
                "b",
                "<break>",
                "e"
            ]
        );
        assert_eq!(words("gh pr \\\nmerge 4"), ["gh", "pr", "merge", "4"]);
        assert_eq!(tokens("gh pr merge 'open"), None);
        assert_eq!(tokens(r#"echo "$(gh pr merge 5""#), None);
    }

    /// A heredoc's body is what its command reads on standard input, one token
    /// beside that command's words, and never read as lines of the shell
    /// (ADR 0188). It ends at the line that is just its delimiter.
    #[test]
    fn a_heredoc_body_belongs_to_its_command_and_is_not_read_as_shell() {
        assert_eq!(
            words("cat <<'EOF' >out | wc\nit's (1\nEOF\necho done"),
            [
                "cat",
                "<stdin:it's (1\n>",
                "<pipe>",
                "wc",
                "<break>",
                "echo",
                "done"
            ]
        );
        assert_eq!(
            words("a <<A && b <<-\"B\"\none\nA\n\t\ttwo\n\tB\nc"),
            [
                "a",
                "<stdin:one\n>",
                "<break>",
                "b",
                "<stdin:two\n>",
                "<break>",
                "c"
            ]
        );
        // Inside a substitution whose output gh keeps as data, the heredoc
        // stays the input of its own command, and its `)` closes nothing.
        assert_eq!(
            words("gh pr create --body \"$(cat <<'BODY'\n1) it's\nBODY\n)\" ; y"),
            [
                "gh",
                "pr",
                "create",
                "--body",
                "<printed:>",
                "<break>",
                "cat",
                "<stdin:1) it's\n>",
                "<break>",
                "<break>",
                "y"
            ]
        );
        // Whose output a shell may run, it is read as commands there.
        assert_eq!(
            words("eval \"$(cat <<'EOF'\ngh pr merge 5\nEOF\n)\""),
            [
                "eval",
                "<printed:>",
                "<break>",
                "cat",
                "<break>",
                "gh",
                "pr",
                "merge",
                "5",
                "<break>",
                "<break>",
                "<break>",
                "<break>"
            ]
        );
        assert_eq!(tokens("x=\"$(cat <<'BODY'\nit's\nBODY\n)\""), None);
        // A gh flag that is not a text flag does not keep its value as text.
        assert!(
            !words("gh api -X PUT \"$(cat <<'B'\nx\nB\n)\"").contains(&"<stdin:x\n>".to_owned())
        );
        assert!(
            words("gh pr create --body \"$(cat <<'B'\nx\nB\n)\"")
                .contains(&"<stdin:x\n>".to_owned())
        );
        // Only gh and git are known to keep a flag's value as text.
        assert_eq!(
            words("echo -m \"$(cat <<'B'\nx\nB\n)\""),
            [
                "echo",
                "-m",
                "<printed:>",
                "<break>",
                "cat",
                "<break>",
                "x",
                "<break>",
                "<break>",
                "<break>",
                "<break>"
            ]
        );
        // `<<` in arithmetic is a shift.
        assert_eq!(
            words("echo $((1<<2)) \"$((3<<1))\" x; (( y <<= 1 )) && z"),
            [
                "echo", "", "", "x", "<break>", "<break>", "<opaque>", "<break>", "z"
            ]
        );
        assert_eq!(tokens("echo $(( $(gh pr merge 5) ))"), None);
        assert_eq!(
            tokens("cat <<EOF\nno end"),
            None,
            "a body the text never closes"
        );
        assert_eq!(tokens("cat <<EOF"), None, "a body that never starts");
        assert_eq!(tokens("echo \"$(cat <<EOF)\"\nx\nEOF"), None);
        assert_eq!(tokens("cat <<"), None, "a heredoc with no delimiter");
        assert_eq!(words("a || b |& c"), ["a", "<break>", "b", "<pipe>", "c"]);
    }

    fn only_merge(command: &str, cwd: &str) -> Merge {
        match merges_in(command, Some(Path::new(cwd)), 0) {
            Parsed::Merges(mut merges) if merges.len() == 1 => merges.remove(0),
            other => panic!("expected one merge in {command:?}, got {other:?}"),
        }
    }

    #[test]
    fn every_shape_of_merge_is_found_with_its_folder_repository_and_head() {
        let merge = only_merge(
            "cd /srv/next && gh pr merge 12 --squash --match-head-commit abc1234def",
            "/home/w",
        );
        assert_eq!(merge.folder.as_deref(), Some(Path::new("/srv/next")));
        assert_eq!(merge.pull_request.as_deref(), Some("12"));
        assert_eq!(merge.repo, None);
        assert_eq!(merge.head, Head::Named("abc1234def".into()));

        let merge = only_merge(
            "gh pr -R miopea/swarm-next merge --match-head-commit=ff00ff0 https://github.com/miopea/swarm-next/pull/7",
            "/home/w",
        );
        assert_eq!(merge.repo.as_deref(), Some("miopea/swarm-next"));
        assert_eq!(
            merge.pull_request.as_deref(),
            Some("https://github.com/miopea/swarm-next/pull/7")
        );
        assert_eq!(merge.head, Head::Named("ff00ff0".into()));

        let merge = only_merge("GH_PAGER= env -i gh pr merge --auto", "/srv/next");
        assert_eq!(merge.pull_request, None);
        assert_eq!(merge.head, Head::Unnamed);
        assert_eq!(merge.folder.as_deref(), Some(Path::new("/srv/next")));

        let merge = only_merge(
            "gh api -X PUT /repos/miopea/swarm-next/pulls/160/merge -f merge_method=squash -fsha=35e3",
            "/",
        );
        assert_eq!(merge.repo.as_deref(), Some("miopea/swarm-next"));
        assert_eq!(merge.pull_request.as_deref(), Some("160"));
        assert_eq!(merge.head, Head::Named("35e3".into()));

        // pflag reads `-iX` as `-i` and then `-X`, which takes the rest of
        // the word or the next one.
        for grouped in [
            "gh api -iX PUT repos/o/r/pulls/5/merge",
            "gh api repos/o/r/pulls/5/merge -iX PUT",
            "gh api repos/o/r/pulls/5/merge -iXput",
        ] {
            let merge = only_merge(grouped, "/");
            assert_eq!(
                (merge.repo.as_deref(), merge.pull_request.as_deref()),
                (Some("o/r"), Some("5")),
                "{grouped}"
            );
        }

        let merge = only_merge(
            "gh api --method=put repos/{owner}/{repo}/pulls/9/merge",
            "/x",
        );
        assert_eq!(merge.repo, None);
        assert_eq!(merge.head, Head::Unnamed);

        let merge = only_merge(
            r#"gh api graphql -f query='mutation { mergePullRequest(input: {pullRequestId: "PR_1", expectedHeadOid: "abc"}) { clientMutationId } }'"#,
            "/",
        );
        assert_eq!(merge.head, Head::Hidden);
        assert!(merge.unreadable.is_some());
        // gh reads a leading `/` and its host's URL as the same endpoint.
        for endpoint in ["/graphql", "https://api.github.com/graphql"] {
            let merge = only_merge(
                &format!(
                    r#"gh api {endpoint} -f query='mutation {{ mergePullRequest(input: {{pullRequestId: "PR_1", expectedHeadOid: "abc"}}) {{ clientMutationId }} }}'"#
                ),
                "/",
            );
            assert_eq!(merge.head, Head::Hidden, "{endpoint}");
        }

        let merge = only_merge(
            r#"curl -sS -X PUT -H "Authorization: Bearer $T" https://api.github.com/repos/miopea/swarm-next/pulls/5/merge -d '{"sha":"abc"}'"#,
            "/",
        );
        assert_eq!(merge.repo.as_deref(), Some("miopea/swarm-next"));
        assert_eq!(merge.head, Head::Hidden);

        for nested in [
            "bash -lc 'cd /srv/next && gh pr merge 3'",
            "eval gh pr merge 3",
            "echo `gh pr merge 3`",
        ] {
            assert_eq!(
                only_merge(nested, "/srv/next").pull_request.as_deref(),
                Some("3")
            );
        }
        // The number arrives on standard input, a word the gate cannot see.
        assert!(unclear("echo 3 | xargs gh pr merge --squash"));

        let merge = only_merge("cd ../next && gh pr merge 1", "/srv/app");
        assert_eq!(merge.folder.as_deref(), Some(Path::new("/srv/app/../next")));
        assert_eq!(only_merge("cd - && gh pr merge 1", "/srv").folder, None);
    }

    #[test]
    fn reading_and_other_commands_are_not_merges() {
        for command in [
            "gh pr view 5 --json mergeable,mergeStateStatus",
            "gh pr merge 5 --disable-auto",
            "gh api repos/miopea/swarm-next/pulls/5/merge",
            "gh api -X GET repos/miopea/swarm-next/pulls/5/merge",
            "git merge origin/main",
            "echo 'gh pr merge 5'",
            "grep -rn mergePullRequest crates",
            "curl https://api.github.com/repos/o/r/pulls/5/merge",
            r#"echo "$(git merge-base a b)""#,
            "gh pr list --search 'is:merged'",
        ] {
            assert_eq!(
                merges_in(command, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{command}"
            );
        }
        assert_eq!(merges_in("gh pr merge 'open", None, 0), Parsed::Unclear);
        assert_eq!(merges_in("echo 'open", None, 0), Parsed::Merges(Vec::new()));
    }

    /// The three commands a worker was refused for: none of them merges,
    /// and "merge" is only in the pull request's text.
    const OPENS_A_PULL_REQUEST: [&str; 3] = [
        "gh pr create --title \"Rename /ship\" --body \"$(cat <<'BODY'\n## Summary\nThe merge-settings check moves; test_merge_settings covers it.\nIt doesn't change what runs (see #48).\n1) rename, 2) docs\nBODY\n)\"",
        "cat > body.md <<'BODY'\nThe merge-settings check moves; test_merge_settings covers it.\nIt doesn't change what runs.\nBODY\ngh pr create --title \"Rename /ship\" --body-file body.md",
        "gh pr create --title \"Rename /ship\" --body-file /tmp/body.md",
    ];

    /// A heredoc's body is read as commands, as a file written from it may
    /// be run later. Only a body that cannot be read as commands, which no
    /// program in its pipeline may run (cat, tee, gh and git keep what they
    /// read as data), is left as data (ADR 0188).
    #[test]
    fn a_heredoc_that_cannot_be_read_is_data_only_where_nothing_may_run_it() {
        for data in OPENS_A_PULL_REQUEST.into_iter().chain([
            "git commit -F - <<'EOF'\nDon't merge yet.\nEOF",
            "git commit --file=- <<'EOF'\nDon't merge yet.\nEOF",
            "git tag -a v1 -F- <<'EOF'\nIt's not merged.\nEOF",
            "cat <<'EOF' | git commit -F -\nDon't merge yet.\nEOF",
            "gh pr comment 5 --body-file - <<'EOF'\nIt's not a merge.\nEOF",
            "gh issue comment 5 -F - <<'EOF'\nIt's not a merge.\nEOF",
            "cat > note.txt <<-'EOF'\n\tIt's not a merge.\n\tEOF\ngh pr view 3",
            "cat <<'EOF' | tee -a log | /usr/bin/cat\nIt's $(gh pr merge 5), unexpanded.\nEOF",
            "gh pr create --body-file - <<'EOF'\nThe merge settings move.\nEOF",
            "gh api graphql -f query='{ viewer { login } }' <<'EOF'\nmutation { mergePullRequest }\nEOF",
        ]) {
            assert_eq!(
                merges_in(data, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{data}"
            );
        }

        for (expanded, number) in [
            (
                "cat <<EOF\nit's merged: $(gh pr merge 5 --match-head-commit abc1234)\nEOF",
                "5",
            ),
            (
                "gh pr comment 5 --body-file - <<EOF\nit's `gh pr merge 6`\nEOF",
                "6",
            ),
        ] {
            assert_eq!(
                only_merge(expanded, "/srv").pull_request.as_deref(),
                Some(number),
                "an unquoted heredoc runs its substitutions: {expanded}"
            );
        }
        assert_eq!(
            merges_in(
                "cat <<EOF\nit's $(gh pr merge 5\nEOF",
                Some(Path::new("/srv")),
                0
            ),
            Parsed::Unclear,
            "a substitution an unquoted body never closes"
        );

        let shell = only_merge(
            "cd /srv/next && bash <<'EOF'\ngh pr merge 3\nEOF",
            "/home/w",
        );
        assert_eq!(shell.pull_request.as_deref(), Some("3"));
        assert_eq!(shell.folder.as_deref(), Some(Path::new("/srv/next")));
        for run in [
            "env A=1 sh -s <<-EOF\n\tgh pr merge 5\n\tEOF",
            "sudo -u w bash <<'EOF'\ngh pr merge 5\nEOF",
            "timeout 60 bash <<'EOF'\ngh pr merge 5\nEOF",
            "ssh host <<'EOF'\ngh pr merge 5\nEOF",
            "docker exec -i c ksh <<'EOF'\ngh pr merge 5\nEOF",
            "cat <<'EOF' | sh\ngh pr merge 5\nEOF",
            "cat > m.sh <<'EOF'\ngh pr merge 5\nEOF\nchmod +x m.sh && ./m.sh",
            "tee m.sh <<'EOF' >/dev/null\ngh pr merge 5\nEOF\nsource m.sh",
            "gh pr comment 5 --body-file - <<'EOF'\n`gh pr merge 5` is the next step.\nEOF",
        ] {
            assert_eq!(
                only_merge(run, "/srv").pull_request.as_deref(),
                Some("5"),
                "a merge that reads as a command in a heredoc counts: {run}"
            );
        }

        for unclear in [
            "bash <<'EOF'\ngh pr merge 3 --subject 'oops\nEOF",
            "cat <<'EOF' | sh\nit's a merge\nEOF",
            // A shell reads what echo prints, which the gate does not follow.
            "echo \"$(cat <<'EOF'\ngh pr merge 5\nEOF\n)\" | bash",
            // Python reads it as code of its own, which the gate does not read.
            "python3 - <<'EOF'\ngh pr merge 5\nEOF",
            "python3 - <<'EOF'\nprint('don't merge')\nEOF",
            "eval \"$(cat <<'EOF'\nit's a merge\nEOF\n)\"",
            // eval parses what bash put in its words, which may be any
            // commands (ADR 0190).
            "eval \"$(cat <<'EOF'\ngh pr merge 5\nEOF\n)\"",
            "x=\"$(cat <<'EOF'\nit's a merge\nEOF\n)\"",
            "gh pr create --body-file - <<'EOF'\nmerge it",
            "gh pr merge 5 --subject \"it's",
        ] {
            assert_eq!(
                merges_in(unclear, Some(Path::new("/srv")), 0),
                Parsed::Unclear,
                "{unclear}"
            );
        }
    }

    /// Refused one way or another: a merge found, or a command not read.
    fn refused(command: &str) -> bool {
        merges_in(command, Some(Path::new("/srv")), 0) != Parsed::Merges(Vec::new())
    }

    /// `((` is arithmetic only where bash reads it so: the parenthesis that
    /// closes the second `(` is followed at once by `)`. Otherwise bash runs
    /// a subshell, and `$((` is a substitution holding one (ADR 0188).
    #[test]
    fn arithmetic_is_only_what_bash_reads_as_arithmetic() {
        for subshell in [
            "((gh pr merge 5) )",
            "x=$((gh pr merge 5) )",
            "echo \"$((gh pr merge 5) )\"",
            "(((gh pr merge 5) ) )",
            "cat <<EOF >/dev/null\n$((gh pr merge 5) )\nit's\nEOF",
        ] {
            assert_eq!(
                only_merge(subshell, "/srv").pull_request.as_deref(),
                Some("5"),
                "{subshell}"
            );
        }
        assert_eq!(
            words("echo $(( (1) << 2 )) \"$((3<<(1)))\" x; (( (y) <<= 1 )) && z"),
            [
                "echo", "", "", "x", "<break>", "<break>", "<opaque>", "<break>", "z"
            ]
        );
        // A name in arithmetic, `${...}` and `$[...]` read a value as
        // arithmetic or as a name, where a subscript runs commands.
        for evaluates in [
            "echo $((x))",
            "echo \"$((_ + 1))\"",
            "(( _ ))",
            "echo $[_]",
            "echo ${!x}",
            "echo \"${x@P}\"",
            "cat <<EOF\n$((_))\nEOF",
            "cat <<EOF\n${!x}\nEOF",
        ] {
            let line = tokens(evaluates).expect(evaluates);
            let opaque = line.contains(&Token::Opaque)
                || line.iter().any(|token| match token {
                    Token::Stdin {
                        body,
                        expands: true,
                        ..
                    } => expansions(body).is_some_and(|run| run.contains(&Token::Opaque)),
                    _ => false,
                });
            assert!(opaque, "{evaluates}");
        }
        assert!(
            !tokens("echo $x \"$y\" $((1 + 2)) '$((z))'")
                .is_some_and(|line| line.contains(&Token::Opaque))
        );
        for unclear in [
            "(( $((gh pr merge 5) ) ))",
            "echo $(( \")\" )) gh pr merge 5",
            "((gh pr merge 5 \"))\" ) )",
        ] {
            assert_eq!(
                merges_in(unclear, Some(Path::new("/srv")), 0),
                Parsed::Unclear,
                "a quote or a substitution in arithmetic is not read: {unclear}"
            );
        }
    }

    /// A heredoc ends at the line bash ends it at, so no line bash runs is
    /// read as its body (ADR 0188).
    #[test]
    fn a_heredoc_ends_where_bash_ends_it() {
        // In double quotes a backslash stays unless it escapes `$`, a
        // backquote, `"`, a backslash or a newline.
        assert_eq!(
            words("cat <<\"E\\OF\"\nEOF\nE\\OF\nx"),
            ["cat", "<stdin:EOF\n>", "<break>", "x"]
        );
        assert_eq!(
            words("cat <<\"E\\$OF\"\nE$OF\nx"),
            ["cat", "<stdin:>", "<break>", "x"]
        );
        assert_eq!(
            words("cat <<\"E\\\nOF\"\nEOF\nx"),
            ["cat", "<stdin:>", "<break>", "x"]
        );
        // Unquoted, a backslash and newline join two lines, before the line
        // is compared with the delimiter and in the body bash passes on.
        assert_eq!(
            words("cat <<EOF\na\\\nb\nE\\\nOF\nx"),
            ["cat", "<stdin:ab\n>", "<break>", "x"]
        );
        assert_eq!(
            words("cat <<-EOF\n\tbody\n\tE\\\n\tOF\n\tEOF\nx"),
            ["cat", "<stdin:body\nE\tOF\n>", "<break>", "x"]
        );
        // Quoted, or with the backslash itself escaped, nothing joins.
        assert_eq!(
            words("cat <<'EOF'\nE\\\nOF\nEOF\nx"),
            ["cat", "<stdin:E\\\nOF\n>", "<break>", "x"]
        );
        assert_eq!(
            words("cat <<EOF\nE\\\\\nOF\nEOF\nx"),
            ["cat", "<stdin:E\\\\\nOF\n>", "<break>", "x"]
        );
        // `$'...'` and `$"..."` delimiters are not read, and inside a
        // substitution bash also ends the heredoc at a line such as `EOF)`.
        for unread in [
            "cat <<$'EOF'\nx\nEOF",
            "cat <<$\"EOF\"\nx\nEOF",
            "cat <<$(echo E)OF\nx\n$\nEOF",
            "cat <<\"${x}\"\nx\n${x}",
            "cat <<`a`\nx\n`a`",
            "x=$(cat <<'EOF'\nbody\nEOF)\necho y\nEOF\n)",
            "x=$( (cat <<'EOF'\nbody\nEOF) )\nEOF\n)",
        ] {
            assert_eq!(tokens(unread), None, "{unread:?}");
        }
        assert_eq!(
            only_merge("cat <<E\\\nOF\nit's $(gh pr merge 5)\nEOF", "/srv")
                .pull_request
                .as_deref(),
            Some("5"),
            "a backslash and newline in the delimiter quote nothing, so the body expands"
        );
        for hidden in [
            "cat <<\"E\\OF\"\n'\nE\\OF\ngh pr merge 5\nEOF",
            "cat <<$'EOF'\n'\nEOF\ngh pr merge 5\n$EOF",
            "cat <<EOF\n'\nE\\\nOF\ngh pr merge 5\nEOF",
            "gh pr create --body \"$(cat <<'EOF'\n'\nEOF)\"\ngh pr merge 5\nEOF\n)\"",
        ] {
            assert!(refused(hidden), "a line bash runs is not body: {hidden}");
        }
    }

    /// A body that cannot be read as commands is data only in a line where
    /// every command is one that runs nothing it is given and no file: cat,
    /// tee, echo and a few like them, gh with its own nouns and git with its
    /// own commands. A substitution's output is words of its command, so a
    /// heredoc in one is data only as the text of a gh or git message, title
    /// or body in double quotes (ADR 0188).
    #[test]
    fn a_body_that_cannot_be_read_is_data_only_in_a_line_that_runs_nothing() {
        let hidden = "gh pr merge 5\nx=$(( $(echo 1) ))";
        for runs in [
            "cat <<'B' > >(bash)\ngh pr merge 5\n'\nB".to_owned(),
            "cat <<'B' | tee >(bash)\ngh pr merge 5\n'\nB".to_owned(),
            "exec > >(bash)\ncat <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            "( cat <<'B' ) | bash\ngh pr merge 5\n'\nB".to_owned(),
            "{\ncat <<'B'\ngh pr merge 5\n'\nB\n} | bash".to_owned(),
            "if true; then\ncat <<'B'\ngh pr merge 5\n'\nB\nfi | bash".to_owned(),
            "f() {\ncat <<'B'\ngh pr merge 5\n'\nB\n}\nf | bash".to_owned(),
            "cat <<'B' $(true) | bash\ngh pr merge 5\n'\nB".to_owned(),
            "mkfifo f; bash < f & cat <<'B' > f\ngh pr merge 5\n'\nB".to_owned(),
            format!("cat > m.sh <<'B'\n{hidden}\nB\nbash m.sh"),
            format!("gh alias set --shell mg - <<'B'\n{hidden}\nB\ngh mg"),
            "gh codespace ssh <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            "git -c alias.x='!bash' x <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            "./cat <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            // `ghci` runs what it reads.
            "gh$(echo ci) pr view <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            "\"gh$(echo ci)\" pr view <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            "\"$(echo bash)\" <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            // What splits can be any of gh's commands, such as `checkout`,
            // which runs git and so its hooks.
            "gh pr $(echo checkout) 5 <<'B'\ngh pr merge 5\n'\nB".to_owned(),
            // bash runs the outer body, which writes a script run after it.
            format!("bash <<'A'\ncat > m.sh <<'B'\n{hidden}\nB\nA\nbash m.sh"),
            // An unquoted body runs its substitutions, here the script.
            format!("cat > m.sh <<'B'\n{hidden}\nB\ncat <<EOF\n$(bash m.sh)\nEOF"),
            // Arithmetic reads a name's value as arithmetic in turn, and a
            // subscript in it runs commands: `$_` holds the last word.
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\necho 'a[$(bash m.sh)]' >/dev/null; echo $((_))"
            ),
            format!("cat > m.sh <<'B'\n{hidden}\nB\necho 'a[$(bash m.sh)]' >/dev/null; echo $[_]"),
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\necho 'a[$(bash m.sh)]' >/dev/null; echo \"${{!_}}\""
            ),
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\necho 'a[$(bash m.sh)]' >/dev/null\ncat <<EOF\n$((_))\nEOF"
            ),
            format!("cat > m.sh <<'B'\n{hidden}\nB\nprintf -v 'a[$(bash m.sh)]' x"),
            // git runs an editor on the standard input it was given, and
            // runs the program `--receive-pack` names.
            "git commit <<'B'\n:!gh pr merge 5\n'\nB".to_owned(),
            "cat <<'B' | git tag -a v1\n:!gh pr merge 5\n'\nB".to_owned(),
            format!("cat > m.sh <<'B'\n{hidden}\nB\ngit push --receive-pack='bash m.sh' . HEAD"),
            "gh pr comment 5 --editor <<'B'\n:!gh pr merge 5\n'\nB".to_owned(),
            // gh runs the editor or browser its config names, which the same
            // command may write.
            "cat > ~/.config/gh/config.yml <<'B'\neditor: sh -c 'gh pr merge 5'\nbrowser: don't-open\nB\ngh pr comment 5 -e".to_owned(),
            "cat > ~/.config/gh/config.yml <<'B'\neditor: sh -c 'gh pr merge 5'\nbrowser: don't-open\nB\ngh pr comment 5 --editor".to_owned(),
            "cat > ~/.config/gh/config.yml <<'B'\nbrowser: sh -c 'gh pr merge 5'\neditor: don't-open\nB\ngh pr view 5 -w".to_owned(),
            "cat > ~/.config/gh/config.yml <<'B'\nbrowser: sh -c 'gh pr merge 5'\neditor: don't-open\nB\ngh repo view --web".to_owned(),
            "cat > ~/.config/gh/config.yml <<'B'\neditor: sh -c 'gh pr merge 5'\nbrowser: don't-open\nB\ngh issue comment 5 -we".to_owned(),
            // `gh gist edit` opens the editor with no flag at all.
            "cat > ~/.config/gh/config.yml <<'B'\neditor: sh -c 'gh pr merge 5'\nbrowser: don't-open\nB\ngh gist edit 1234".to_owned(),
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\ngh repo clone o/r -- --upload-pack='bash m.sh'"
            ),
            // The same, given the body as a file on standard input, or a
            // flag git takes by its start, or a printed flag or `--`.
            "cat > m.txt <<'B'\n:!gh pr merge 5\n'\nB\ngit commit < m.txt".to_owned(),
            "cat > m.txt <<'B'\n:!gh pr merge 5\n'\nB\ngh pr comment 5 --editor < m.txt".to_owned(),
            "git commit -p -F - <<'B'\ne\n:!gh pr merge 5\n'\nB".to_owned(),
            "git commit --inter -F - <<'B'\n:!gh pr merge 5\n'\nB".to_owned(),
            // Each layer on its own, with nothing given on standard input:
            // an unquoted substitution after a text flag splits into flags,
            // a printed part of a name (`rpm`, read as `rm`), and gh's and
            // git's commands that run what they are named.
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\ngh repo clone o/r --title $(echo -- -u 'bash m.sh')"
            ),
            format!("cat > m.sh <<'B'\n{hidden}\nB\nr$(echo p)m --eval '%(bash m.sh)'"),
            format!("cat > m.sh <<'B'\n{hidden}\nB\ngh alias set --shell mg 'bash m.sh' && gh mg"),
            format!("cat > m.sh <<'B'\n{hidden}\nB\ngit -c alias.x='!bash m.sh' x"),
            format!("cat > m.sh <<'B'\n{hidden}\nB\ngit push --rec='bash m.sh' . HEAD"),
            format!("cat > m.sh <<'B'\n{hidden}\nB\ngit push \"$(echo --receive-pack=x)\" . HEAD"),
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\ngh repo clone o/r \"$(echo --)\" -u 'bash m.sh'"
            ),
            format!("cat > m.sh <<'B'\n{hidden}\nB\nprintf \"$(echo '-va[$(bash m.sh)]')\" x"),
            "gh $(cat <<'B'\npr merge 5 --subject '\nB\n)".to_owned(),
            "gh api -X PUT \"$(cat <<'B'\nrepos/o/r/pulls/5/merge?x='\nB\n)\"".to_owned(),
            "gh pr merge 5 --body $(cat <<'B'\nx --repo o/r '\nB\n)".to_owned(),
        ] {
            assert_eq!(
                merges_in(&runs, Some(Path::new("/srv")), 0),
                Parsed::Unclear,
                "{runs}"
            );
        }
        for data in [
            "cd /srv && git add a.md && git commit -m \"$(cat <<'EOF'\nfix: it isn't a merge\nEOF\n)\" && git push",
            "gh pr create --title t --body \"$(cat <<'EOF'\nIt's the merge fix\nEOF\n)\" --base main",
            "gh pr create --title t --body=\"$(cat <<'EOF'\nIt's the merge fix\nEOF\n)\"",
            "gh issue comment 5 -b \"Summary: $(cat <<'EOF'\nit's merged\nEOF\n)\"",
            "mkdir -p docs && cat > docs/x.md <<'EOF'\nThe merge gate's rule.\nEOF",
            "gh api repos/o/r/issues/5/comments -F body=@- <<'EOF'\nIt's not a merge.\nEOF",
        ] {
            assert_eq!(
                merges_in(data, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{data}"
            );
        }
        let merge = only_merge(
            "gh pr merge 5 --squash -b \"$(cat <<'EOF'\nIt's merged.\nEOF\n)\" --match-head-commit abc1234",
            "/srv",
        );
        assert_eq!(
            (merge.pull_request.as_deref(), merge.head),
            (Some("5"), Head::Named("abc1234".into())),
            "the words after a substitution stay its command's"
        );
    }

    /// Which letter of a group of gh's one-letter flags takes the rest as
    /// its value depends on the command (`-f` is `--fill` to `gh pr create`
    /// and takes a value in `gh api`), so any group holding an `e` or a `w`
    /// may open the editor or browser gh's config names (ADR 0188). A value
    /// given as its own word is not a group, nor is a text flag's value.
    #[test]
    fn any_group_of_gh_flags_with_an_e_or_a_w_may_open_a_program() {
        let browser = "cat > ~/.config/gh/config.yml <<'B'\nbrowser: sh -c 'gh pr merge 5'\neditor: don't-open\nB\n";
        let editor = "cat > ~/.config/gh/config.yml <<'B'\neditor: sh -c 'gh pr merge 5'\nbrowser: don't-open\nB\n";
        let note = "cat > notes.md <<'B'\nIt's the merge note.\nB\n";
        for runs in [
            format!("{browser}gh pr create -fw"),
            format!("{editor}gh pr create -fe"),
            format!("{browser}gh gist create -pw f.txt"),
            format!("{browser}gh pr view 5 -wq.title"),
            format!("{browser}gh pr view 5 -c=t -w"),
        ] {
            assert_eq!(
                merges_in(&runs, Some(Path::new("/srv")), 0),
                Parsed::Unclear,
                "{runs}"
            );
        }
        for data in [
            format!("{note}gh pr comment 5 -R o/web --body-file notes.md"),
            format!("{note}gh pr comment 5 --body \"- fixed the web view\""),
            // `-R`, `-F` and `-q` take a value in every gh command, so what
            // follows them in the group is that value.
            format!("{note}gh pr comment 5 -Ro/web --body-file notes.md"),
            format!("{note}gh pr comment 5 -Fnotes.md"),
            format!("{note}gh pr comment 5 -Rwebteam/site --body-file notes.md"),
            format!("{note}gh pr comment 5 -Fweekly.md"),
            format!("{note}gh pr comment 5 -Rowner/the-website -F notes.md"),
            format!("{note}gh pr view 5 --json mergeable -q.mergeable"),
            // A letter given its value with `=` ends the group.
            format!("{note}gh pr comment 5 -f=web --body-file notes.md"),
        ] {
            assert_eq!(
                merges_in(&data, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{data}"
            );
        }
    }

    /// What a substitution prints is a message's text only as the value of a
    /// text flag, of a gh or git command that takes text, where gh or git
    /// reads that flag as a flag (ADR 0188). After `--template`, `-q`, `-o`
    /// or `--trailer`, `-b` or `-m` is that flag's value, and the printed
    /// word is an endpoint or a flag.
    #[test]
    fn a_printed_word_is_text_only_as_the_value_of_a_flag_read_as_one() {
        let keeps = |text: &str| words(text).contains(&"<stdin:x\n>".to_owned());
        // The flag is first, or after a word that is not a flag, a flag with
        // its value attached, a flag that takes none, or a text flag's value.
        for kept in [
            "git commit -a -m \"$(cat <<'B'\nx\nB\n)\"",
            "gh pr comment 5 --body \"$(cat <<'B'\nx\nB\n)\"",
            "gh pr create --base=main -b \"$(cat <<'B'\nx\nB\n)\"",
            "gh pr create -t \"$(echo t)\" -b \"$(cat <<'B'\nx\nB\n)\"",
            "gh pr create --draft --body=\"$(cat <<'B'\nx\nB\n)\"",
        ] {
            assert!(keeps(kept), "{kept}");
        }
        for read in [
            "gh api -X PUT -b \"$(cat <<'B'\nx\nB\n)\"",
            "gh pr checkout -b \"$(cat <<'B'\nx\nB\n)\"",
            "git push -m \"$(cat <<'B'\nx\nB\n)\"",
            "gh pr create --base \"$(cat <<'B'\nx\nB\n)\"",
            "gh pr create --base=\"$(cat <<'B'\nx\nB\n)\"",
            "git commit --trailer -m \"$(cat <<'B'\nx\nB\n)\"",
            "git commit \"$(echo --trailer)\" -m \"$(cat <<'B'\nx\nB\n)\"",
            "git commit -- x -m \"$(cat <<'B'\nx\nB\n)\"",
            "git commit --end-of-options x -m \"$(cat <<'B'\nx\nB\n)\"",
            // A program or command that a substitution prints part of may be
            // another one, such as `git-x`.
            "\"git$(echo -x)\" commit -m \"$(cat <<'B'\nx\nB\n)\"",
            "gh \"pr$(echo x)\" create -b \"$(cat <<'B'\nx\nB\n)\"",
        ] {
            assert!(!keeps(read), "{read}");
        }

        let hidden = "gh pr merge 5\nx=$(( $(echo 1) ))";
        for runs in [
            "gh api -X PUT --template -b \"$(cat <<'B'\nrepos/o/r/pulls/5/merge?x='\nB\n)\""
                .to_owned(),
            "gh api -X PUT -q -b \"$(cat <<'B'\nrepos/o/r/pulls/5/merge?x='\nB\n)\"".to_owned(),
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\ngit push -o -m \"$(echo --receive-pack=bash m.sh)\" . HEAD"
            ),
            "git commit -F - --trailer -m \"$(echo --patch)\" <<'B'\ne\n:!gh pr merge 5\n'\nB"
                .to_owned(),
            // A word printed before a flag's name is not that flag's value.
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\ngit push \"$(printf '%s' '--receive-pack=bash m.sh ')\"--message=x . HEAD"
            ),
            // Each on its own: `git fetch -m` takes no value, an unquoted
            // substitution in a text flag's value splits, and a word is text
            // only when every substitution in it is.
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\ngit fetch -m \"$(echo --upload-pack=bash m.sh)\" ."
            ),
            format!("cat > m.sh <<'B'\n{hidden}\nB\ngit commit -m $(echo x --patch)"),
            format!(
                "cat > m.sh <<'B'\n{hidden}\nB\ngit commit \"$(echo --patch)\"--message=\"$(echo x)\""
            ),
        ] {
            assert_eq!(
                merges_in(&runs, Some(Path::new("/srv")), 0),
                Parsed::Unclear,
                "{runs}"
            );
        }
        for data in [
            "git commit -a -m \"$(cat <<'EOF'\nDon't merge yet\nEOF\n)\"",
            "gh pr create --draft --title \"$(cat <<'EOF'\nIt's the merge fix\nEOF\n)\" --body \"$(cat <<'EOF'\nIt's the merge fix\nEOF\n)\"",
            "gh pr comment 5 --body \"$(cat <<'EOF'\nIt's not a merge\nEOF\n)\"",
            "gh pr create --base=main -b \"$(cat <<'EOF'\nIt's the merge fix\nEOF\n)\"",
        ] {
            assert_eq!(
                merges_in(data, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{data}"
            );
        }
    }

    /// A word bash computes as it runs the command may be any word, as what a
    /// substitution prints may be: a variable, `~`, `$"..."`, a brace list
    /// or a glob. For gh, git and printf it counts only as the quoted value
    /// of a text flag (ADR 0188). `$'...'` is text bash makes as it reads
    /// the command (ADR 0190).
    #[test]
    fn a_word_bash_computes_counts_only_as_a_text_flags_value() {
        assert_eq!(
            words("echo \"$x\" $y ~ a* b? c[d] {e,f} -{g..h} $'i' $\"j\" k$'\\''"),
            [
                "echo",
                "<printed:$x>",
                "<split:$y>",
                "<printed:~>",
                "<split:a*>",
                "<split:b?>",
                "<split:c[d]>",
                "<split:{e,f}>",
                "<split:-{g..h}>",
                "i",
                "<printed:$j>",
                "k'",
            ]
        );
        // A brace with no list, a `[` with no `]`, a `~` not first, and what
        // is quoted are as written.
        assert_eq!(
            words("gh api repos/{owner}/{repo} [ x] x~ '*' \\* \"$\" $ ''~ {x} {x},y {x.y} y{ a,b"),
            [
                "gh",
                "api",
                "repos/{owner}/{repo}",
                "[",
                "x]",
                "x~",
                "*",
                "*",
                "$",
                "$",
                "~",
                "{x}",
                "{x},y",
                "{x.y}",
                "y{",
                "a,b",
            ]
        );
        // `$'...'` ends at its own quote, which a backslash escapes.
        assert_eq!(words("echo $'\\'' ; x #'"), ["echo", "'", "<break>", "x"]);
        // With extglob on, `!(x)` and its like are patterns, not subshells.
        for pattern in ["ls !(x)", "ls @(a|b)", "ls x+(y)", "ls *(z)", "ls ?(z)"] {
            assert_eq!(tokens(pattern), None, "{pattern}");
        }
        assert!(tokens("f() { :; }").is_some());

        let hidden = "cat > m.sh <<'B'\ngh pr merge 5\nx=$(( $(echo 1) ))\nB\n";
        let file = "cat > '--receive-pack=bash m.sh' < /dev/null\n";
        let editor = "<<'B'\ne\n:!gh pr merge 5\n'\nB";
        for runs in [
            format!("{hidden}git push {{\"--receive-pack=bash m.sh\",}} . HEAD"),
            format!("{hidden}git push $'--receive-pack=bash m.sh' . HEAD"),
            format!("{hidden}git push --receive-pa$'ck=bash m.sh' . HEAD"),
            format!("{hidden}git push $\"--receive-pack=bash m.sh\" . HEAD"),
            format!("{hidden}{file}git push --rece* . HEAD"),
            format!("{hidden}{file}git push --receive-pac[k]='bash m.sh' . HEAD"),
            format!("{hidden}{file}git push . HEAD !(cat)"),
            format!("{hidden}: --receive-pack='bash m.sh'; git push \"$_\" . HEAD"),
            format!("{hidden}: '--receive-pack=bash m.sh'; git push $_ . HEAD"),
            format!("{hidden}printf {{-v,}} 'a[$(bash m.sh)]' x"),
            format!("{hidden}gh repo clone o/r {{--,}} -u 'bash m.sh'"),
            format!("git commit -F - {{--trailer,}} -m \"$(echo --patch)\" {editor}"),
            format!("git commit -F - $'-p' {editor}"),
            format!("git commit -F - -{{o..q}} {editor}"),
        ] {
            assert_eq!(
                merges_in(&runs, Some(Path::new("/srv")), 0),
                Parsed::Unclear,
                "{runs}"
            );
        }
        for data in [
            "gh pr create --title \"$TITLE\" --body \"$(cat <<'EOF'\nIt's the merge fix\nEOF\n)\"",
            "git commit -m \"$(cat <<'EOF'\nDon't merge yet\nEOF\n)\" -m \"Refs $T\"",
            "gh api repos/{owner}/{repo}/issues/5/comments -F body=@- <<'EOF'\nIt's not a merge\nEOF",
            "cd ~/x && git commit -F - <<'EOF'\nDon't merge yet\nEOF",
            "ls \"$HOME\"/*.md && cat > x.md <<'EOF'\nIt's the merge fix\nEOF",
        ] {
            assert_eq!(
                merges_in(data, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{data}"
            );
        }
    }

    /// gh and curl send a heredoc as their request when told to read it:
    /// `--input -` and `-d @-`, or piped from cat (ADR 0188).
    #[test]
    fn gh_and_curl_read_their_request_from_a_heredoc() {
        let with_body = only_merge(
            "gh pr merge 7 --match-head-commit 35e32938 --body-file - <<'EOF'\nIt's merged.\nEOF",
            "/srv",
        );
        assert_eq!(with_body.head, Head::Named("35e32938".into()));
        let graphql = only_merge(
            "gh api graphql --input - <<'EOF'\n{\"query\": \"mutation { mergePullRequest(input: {pullRequestId: $id, expectedHeadOid: $oid}) { clientMutationId } }\"}\nEOF",
            "/srv",
        );
        assert_eq!(graphql.head, Head::Hidden);
        assert!(graphql.unreadable.is_some());
        let piped = only_merge(
            "cat <<'EOF' | gh api graphql --input -\n{\"query\": \"mutation { mergePullRequest(input: {clientMutationId: \\\"it's\\\"}) { clientMutationId } }\"}\nEOF",
            "/srv",
        );
        assert!(
            piped.unreadable.is_some(),
            "gh reads the request piped to it"
        );
        let rest = only_merge(
            "gh api -X PUT repos/miopea/swarm-next/pulls/5/merge --input - <<'EOF'\n{\"sha\": \"35e32938\"}\nEOF",
            "/srv",
        );
        assert_eq!(rest.head, Head::Hidden);
        let posted = only_merge(
            "curl -X PUT https://api.github.com/repos/miopea/swarm-next/pulls/5/merge -d @- <<'EOF'\n{\"sha\": \"35e32938\"}\nEOF",
            "/srv",
        );
        assert_eq!(posted.head, Head::Hidden);
    }

    /// A GraphQL request gh or curl reads from a file, or from standard input
    /// that is not a heredoc passed on as written, may be a merge the gate
    /// cannot read, so it is held as one: the command must give the request
    /// itself (ADR 0188).
    #[test]
    fn a_graphql_request_the_gate_cannot_see_is_held_as_a_merge() {
        let mutation =
            "mutation { mergePullRequest(input: {pullRequestId: \"X\"}) { clientMutationId } }";
        let json = "{\"query\": \"mutation { mergePullRequest(input: {pullRequestId: \\\"X\\\"}) { clientMutationId } }\", \"note\": \"don't\"}";
        let escaped = "{\"query\": \"mutation { merge\\u0050ullRequest(input: {pullRequestId: \\\"X\\\"}) { clientMutationId } }\", \"note\": \"don't\"}";
        let innocent = "{\"query\": \"query { viewer { login } }\"}";
        for unseen in [
            format!("cat > q.graphql <<'B'\n{mutation}#don't\nB\ngh api graphql -F query=@q.graphql"),
            format!("cat > q.json <<'B'\n{json}\nB\ngh api graphql --input q.json"),
            format!("cat > q.json <<'B'\n{json}\nB\ngh api graphql --input - < q.json"),
            format!(
                "tee q.graphql > /dev/null <<'B'\n{mutation}#don't\nB\ngh api graphql -F query=@q.graphql"
            ),
            format!(
                "cat > q.graphql <<'B'\n{mutation}#don't\nB\ncat q.graphql | gh api graphql -F query=@-"
            ),
            format!("cat > q.json <<'B'\n{escaped}\nB\ngh api graphql --input q.json"),
            // Readable as shell, or written by printf: the file is still the
            // request, and the gate does not read it.
            format!("cat > q.graphql <<'B'\n{mutation}\nB\ngh api graphql -F query=@q.graphql"),
            format!("printf '%s' '{mutation}' > q.graphql && gh api graphql -F query=@q.graphql"),
            "gh api graphql --field query=@q.graphql".to_owned(),
            "gh api graphql -Fquery=@q.graphql".to_owned(),
            // The endpoint with a leading `/`, or as its host's URL.
            format!("cat > q.graphql <<'B'\n{mutation}#don't\nB\ngh api /graphql -F query=@q.graphql"),
            "gh api /graphql --input q.json".to_owned(),
            "gh api https://api.github.com/graphql --input q.json".to_owned(),
            // `-F` after a flag that takes no value, in one group.
            "gh api graphql -iF query=@q.graphql".to_owned(),
            "gh api graphql -iFquery=@q.graphql".to_owned(),
            // pflag takes one `=` off a one-letter flag's value.
            "gh api graphql -F=query=@q.graphql".to_owned(),
            "gh api graphql -iF=query=@q.graphql".to_owned(),
            // A heredoc on another descriptor is not standard input.
            format!("cat <<'A' | gh api graphql --input - 3<<'C'\n{json}\nA\nx\nC"),
            format!("cat <<'A' | gh api graphql -F query=@- 3<<'C'\n{mutation}#don't\nA\nx\nC"),
            format!("cat q.json | gh api graphql --input - 3<<'C'\n{innocent}\nC"),
            format!("gh api graphql --input - 3<<'C'\n{innocent}\nC"),
            format!("gh api graphql --input /dev/fd/0 <<'B'\n{innocent}\nB"),
            // Standard input from a here-string, a file named after the
            // heredoc, a command that changes or prints what it passes on,
            // or nothing at all.
            "gh api graphql --input - <<< \"$request\"".to_owned(),
            format!("cat <<'B' | gh api graphql --input - <<< \"$request\"\n{innocent}\nB"),
            format!("gh api graphql --input - <<'B' < q.json\n{innocent}\nB"),
            // cat prints a file it is named, not its heredoc.
            format!("cat q.graphql <<'B' | gh api graphql --input -\n{innocent}\nB"),
            "sed 's/x/merge/' <<'B' | gh api graphql --input -\n{\"query\": \"mutation { xPullRequest(input: {pullRequestId: \\\"X\\\"}) { clientMutationId } }\"}\nB".to_owned(),
            "printf '%s' \"$request\" | gh api graphql --input -".to_owned(),
            "gh api graphql --input -".to_owned(),
            "curl -X POST https://api.github.com/graphql -d @q.json".to_owned(),
            "curl https://api.github.com/graphql --json @q.json".to_owned(),
            "curl https://api.github.com/graphql -d @- < q.json".to_owned(),
            "curl -K request.cfg".to_owned(),
        ] {
            let merge = only_merge(&unseen, "/srv");
            assert_eq!(merge.head, Head::Unnamed, "{unseen}");
            assert!(merge.unreadable.is_some(), "{unseen}");
        }
        for seen in [
            format!("gh api graphql --input - <<'B'\n{innocent}\nB"),
            format!("gh api graphql --input - 0<<'B'\n{innocent}\nB"),
            format!("cat <<'B' | gh api graphql --input -\n{innocent}\nB"),
            format!("cat <<'B' | tee request.json | gh api graphql --input -\n{innocent}\nB"),
            // `-f` sends an `@` as it is, grouped or not.
            "gh api graphql -f query=@q.graphql".to_owned(),
            "gh api graphql -if query=@q.graphql".to_owned(),
            "gh api /graphql -f query='query { viewer { login } }'".to_owned(),
            "gh api graphql -f query='query { viewer { login } }'".to_owned(),
            // Only a GraphQL request can hide a merge: a REST merge names
            // its endpoint.
            "gh api repos/o/r/issues/5/comments --input body.json".to_owned(),
            format!("curl https://api.github.com/graphql -d @- <<'B'\n{innocent}\nB"),
            "curl https://api.github.com/graphql --data-raw @q.json".to_owned(),
            "curl -d @body.json https://api.github.com/repos/o/r/issues/5/comments".to_owned(),
        ] {
            assert_eq!(
                merges_in(&seen, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{seen}"
            );
        }
    }

    /// bash gives a heredoc to standard input unless a descriptor stands
    /// right before `<<`: digits no quote or backslash touches, or a
    /// `{name}`, for which bash opens a new one. A quoted or escaped digit,
    /// or a braced word that is not a name, is a word of the command (ADR
    /// 0188).
    #[test]
    fn a_heredoc_is_standard_input_unless_bash_reads_a_descriptor_before_it() {
        assert_eq!(
            words(
                "cat - {1x}<<'H' \"3\"<<'C' \\4<<'D' '6'<<'G' {fd}<<'E' 5<<'F'\nh\nH\nc\nC\nd\nD\ng\nG\ne\nE\nf\nF"
            ),
            [
                "cat",
                "-",
                "{1x}",
                "<stdin:h\n>",
                "3",
                "<stdin:c\n>",
                "4",
                "<stdin:d\n>",
                "6",
                "<stdin:g\n>",
                "<stdin:e\n>",
                "<stdin:f\n>",
                "<break>"
            ]
        );
        let mutation =
            "mutation { mergePullRequest(input: {pullRequestId: \"X\"}) { clientMutationId } }";
        let json = "{\"query\": \"mutation { mergePullRequest(input: {pullRequestId: \\\"X\\\"}) { clientMutationId } }\", \"note\": \"don't\"}";
        let innocent = "{\"query\": \"query { viewer { login } }\"}";
        let viewer = "query { viewer { login } }";
        for held in [
            // A quoted or escaped digit: the heredoc is standard input, and
            // what is piped is not.
            format!(
                "cat <<'A' | gh api graphql -F query=@- --jq \"3\"<<'C'\n{viewer}\nA\n{mutation}#don't\nC"
            ),
            format!(
                "cat <<'A' | gh api graphql -F query=@- --jq \\3<<'C'\n{viewer}\nA\n{mutation}#don't\nC"
            ),
            format!("cat <<'A' | gh api graphql --input - --jq '3'<<'C'\n{innocent}\nA\n{json}\nC"),
            format!(
                "cat <<'A' | curl -d @- https://api.github.com/graphql \"3\"<<'C'\n{innocent}\nA\n{json}\nC"
            ),
            // A named descriptor: what is piped is standard input, and the
            // heredoc is not.
            format!(
                "cat <<'A' | gh api graphql -F query=@- {{fd}}<<'C'\n{mutation}#don't\nA\nx\nC"
            ),
            format!("cat <<'A' | gh api graphql --input - {{fd}}<<'C'\n{json}\nA\nx\nC"),
            format!("cat q.json | gh api graphql --input - {{fd}}<<'C'\n{innocent}\nC"),
            format!(
                "cat q.json | curl -d @- https://api.github.com/graphql {{fd}}<<'C'\n{innocent}\nC"
            ),
        ] {
            assert!(only_merge(&held, "/srv").unreadable.is_some(), "{held}");
        }
        let piped =
            format!("cat <<'A' | gh api graphql --input - {{fd}}<<'C'\n{innocent}\nA\nx\nC");
        assert_eq!(
            merges_in(&piped, Some(Path::new("/srv")), 0),
            Parsed::Merges(Vec::new()),
            "{piped}"
        );
    }

    /// bash reads `{a[1]}` before a redirection as an array element to set
    /// to a new descriptor, and `{1x}` or `{}` as a word of the command, with
    /// the heredoc on standard input. Which `[...]` bash takes as an element
    /// is its own parser's to say, so a braced word holding a `[` there is
    /// not read; any other is a word, as bash reads it (ADR 0188).
    #[test]
    fn a_braced_word_that_may_be_an_array_element_before_a_redirection_is_not_read() {
        let mutation =
            "mutation { mergePullRequest(input: {pullRequestId: \"X\"}) { clientMutationId } }";
        let readable = "{\"query\": \"mutation { mergePullRequest(input: {pullRequestId: \\\"X\\\"}) { clientMutationId } }\"}";
        let innocent = "{\"query\": \"query { viewer { login } }\"}";
        for unread in [
            format!("cat <<'A' | gh api graphql --input - {{a[1]}}<<'C'\n{readable}\nA\nx\nC"),
            format!("cat <<'A' | gh api graphql -F query=@- {{a[x+1]}}<<'C'\n{mutation}\nA\nx\nC"),
            format!("cat <<'A' | gh api graphql --input - {{a[1]}}< q.json\n{readable}\nA"),
            "gh {a[1]}<<'C' pr merge 5\nx\nC".to_owned(),
            "gh {a[1]}>/dev/null pr merge 5".to_owned(),
        ] {
            assert_eq!(
                merges_in(&unread, Some(Path::new("/srv")), 0),
                Parsed::Unclear,
                "{unread}"
            );
        }
        assert_eq!(tokens("cat - {a[1]}<<'H'\nh\nH"), None);
        // bash takes only a word that starts with `{` and ends with `}`.
        for word in ["{a[1]x", "x{a[1]}"] {
            assert!(
                tokens(&format!("cat - {word}<<'H'\nh\nH")).is_some(),
                "{word}"
            );
        }
        for heredoc_is_input in [
            format!(
                "cat <<'A' | gh api graphql --input - {{1x}}<<'C'\n{innocent}\nA\n{readable}\nC"
            ),
            format!("cat <<'A' | gh api graphql --input - {{}}<<'C'\n{innocent}\nA\n{readable}\nC"),
        ] {
            assert!(
                only_merge(&heredoc_is_input, "/srv").unreadable.is_some(),
                "{heredoc_is_input}"
            );
        }
    }

    /// pflag ends a group of one-letter flags at an `=`, and the letter
    /// before it takes the rest of the word, even one that takes no value:
    /// `-i=t` is `--include` set to true, and `-X PUT` after it is read as
    /// ever (ADR 0188).
    #[test]
    fn an_equals_sign_ends_a_group_of_one_letter_flags() {
        for grouped in [
            "gh api -i=t -X PUT repos/o/r/pulls/5/merge",
            "gh api -i=f -X PUT repos/o/r/pulls/5/merge",
            "gh api -i=F -X PUT repos/o/r/pulls/5/merge",
            "gh api repos/o/r/pulls/5/merge -i=true -X PUT",
        ] {
            let merge = only_merge(grouped, "/");
            assert_eq!(
                (merge.repo.as_deref(), merge.pull_request.as_deref()),
                (Some("o/r"), Some("5")),
                "{grouped}"
            );
        }
        let merge = only_merge(
            r#"gh api -i=t graphql -f query='mutation { mergePullRequest(input: {pullRequestId: "PR_1"}) { clientMutationId } }'"#,
            "/",
        );
        assert!(merge.unreadable.is_some());
        assert!(
            only_merge("gh api -i=t graphql -F query=@q.graphql", "/")
                .unreadable
                .is_some()
        );
        // A letter that takes a value keeps the rest of the word, less one
        // `=`, as before.
        let merge = only_merge("gh api -X=PUT repos/o/r/pulls/5/merge", "/");
        assert_eq!(merge.pull_request.as_deref(), Some("5"));
    }

    /// `3>&0 <<'C' 0>&3` saves the pipe, gives the heredoc to standard input
    /// and then puts the pipe back: a dup onto descriptor 0 leaves standard
    /// input to whatever descriptor it names, which the gate does not follow
    /// (ADR 0188). A dup of any other descriptor leaves it alone.
    #[test]
    fn a_dup_onto_descriptor_0_leaves_standard_input_unseen() {
        let mutation =
            "mutation { mergePullRequest(input: {pullRequestId: \"X\"}) { clientMutationId } }";
        let json = "{\"query\": \"mutation { mergePullRequest(input: {pullRequestId: \\\"X\\\"}) { clientMutationId } }\", \"note\": \"it`s\"}";
        let readable = "{\"query\": \"mutation { mergePullRequest(input: {pullRequestId: \\\"X\\\"}) { clientMutationId } }\"}";
        let innocent = "{\"query\": \"query { viewer { login } }\"}";
        for held in [
            format!("cat <<'A' | gh api graphql --input - 3>&0 <<'C' 0>&3\n{json}\nA\nx\nC"),
            format!(
                "cat <<'A' | gh api graphql -F query=@- 3>&0 <<'C' 0>&3\n{mutation}#don't\nA\nx\nC"
            ),
            format!(
                "cat <<'A' | gh api graphql --input - 3>&0 <<'C' 0>&3\n{readable}\nA\n{innocent}\nC"
            ),
            format!(
                "cat q.json | curl -d @- https://api.github.com/graphql 3>&0 <<'C' 0>&3\n{innocent}\nC"
            ),
        ] {
            assert!(only_merge(&held, "/srv").unreadable.is_some(), "{held}");
        }
        for seen in [
            format!("cat <<'B' | gh api graphql --input - 2>&1\n{innocent}\nB"),
            format!("gh api graphql --input - <<'B' >&2\n{innocent}\nB"),
        ] {
            assert_eq!(
                merges_in(&seen, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{seen}"
            );
        }
    }

    /// Whether GitHub ignores the case of its GraphQL endpoint's path was
    /// not run, so the endpoint is held in any case (ADR 0188).
    #[test]
    fn the_graphql_endpoint_is_held_in_any_case_of_its_letters() {
        for held in [
            "gh api GraphQL -F query=@q.graphql",
            "gh api /GRAPHQL --input q.json",
            "curl -d @q.json https://api.github.com/GraphQL",
        ] {
            assert!(only_merge(held, "/srv").unreadable.is_some(), "{held}");
        }
    }

    const MUTATION: &str = r#"{"query":"mutation { mergePullRequest(input:{pullRequestId:\"x\"}) { clientMutationId } }"}"#;
    const INNOCENT: &str = r#"{"query":"query { viewer { login } }"}"#;

    fn unclear(command: &str) -> bool {
        merges_in(command, Some(Path::new("/srv")), 0) == Parsed::Unclear
    }

    /// The program is read past the reserved words bash reads a command
    /// after, and past wrappers that run the rest as it is (ADR 0190).
    #[test]
    fn a_merge_after_a_reserved_word_or_a_wrapper_is_read() {
        for command in [
            "{ gh pr merge 5; }",
            "if true; then gh pr merge 5; fi",
            "! gh pr merge 5",
            "while true; do gh pr merge 5; break; done",
            "if false; then :; else gh pr merge 5; fi",
            "if false; then :; elif true; then gh pr merge 5; fi",
            "until false; do gh pr merge 5; done",
            "time -p gh pr merge 5",
            "timeout 60 gh pr merge 5",
            "timeout -s KILL -k 5 60 gh pr merge 5",
            "/usr/bin/timeout --signal=KILL 60 gh pr merge 5",
            "nice -n 5 gh pr merge 5",
            "exec -a x gh pr merge 5",
            "stdbuf -oL gh pr merge 5",
            "stdbuf -o L gh pr merge 5",
            "command gh pr merge 5",
            "/usr/bin/env gh pr merge 5",
            "nohup gh pr merge 5",
            // An empty array, which is not a function.
            "args=(); gh pr merge 5",
        ] {
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.pull_request.as_deref(), Some("5"), "{command}");
            assert_eq!(
                merge.folder.as_deref(),
                Some(Path::new("/srv")),
                "{command}"
            );
            assert_eq!(merge.unreadable, None, "{command}");
        }
        // env runs its command in the folder it names, or splits its string
        // into the command and its first words.
        for (command, folder) in [
            ("env -C /tmp gh pr merge 5", "/tmp"),
            ("env --chdir=/tmp gh pr merge 5", "/tmp"),
            ("env -S 'gh pr merge 5'", "/srv"),
            ("env -S 'gh pr' merge 5", "/srv"),
            // A wrapper's `cd` moves nothing here; `command`'s does.
            ("env -C /tmp cd /x; gh pr merge 5", "/srv"),
            ("command cd /x; gh pr merge 5", "/x"),
        ] {
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.pull_request.as_deref(), Some("5"), "{command}");
            assert_eq!(
                merge.folder.as_deref(),
                Some(Path::new(folder)),
                "{command}"
            );
        }
        for command in [
            "timeout 60 cargo test merge_gate",
            "nice -n 5 cargo build",
            "for f in a b; do echo $f; done",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// A program the gate does not know may run gh among its words, as
    /// another account or elsewhere: the merge is read, and is not one
    /// Swarm can ask gh about (ADR 0190).
    #[test]
    fn a_merge_another_program_runs_is_read_and_cannot_be_checked() {
        for command in [
            "sudo -u w gh pr merge 5",
            "sudo gh pr merge 5",
            "watch gh pr merge 5",
            "flock /tmp/l gh pr merge 5",
            "ssh host gh pr merge 5",
            "find . -name x -exec gh pr merge 5 \\;",
            "sudo -u w bash -c 'gh pr merge 5'",
            "ssh host <<'EOF'\ngh pr merge 5\nEOF",
        ] {
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.pull_request.as_deref(), Some("5"), "{command}");
            assert_eq!(merge.unreadable, Some(THROUGH_ANOTHER_PROGRAM), "{command}");
        }
        // A command bash computes for a program that runs one may be the
        // merge.
        assert!(unclear("c='gh pr merge 5'; sudo $c"));
    }

    /// A variable gh reads to choose the repository, the account or the
    /// host, set or cleared in the command, is not one the gate's own gh
    /// sees, so the merge is not one it can check (ADR 0190).
    #[test]
    fn a_variable_gh_reads_to_choose_what_it_merges_leaves_the_merge_unchecked() {
        for command in [
            "GH_REPO=o/r gh pr merge 5",
            "GH_TOKEN=x gh pr merge 5",
            "GH_HOST=example.com gh pr merge 5",
            "export GH_REPO=o/r; gh pr merge 5",
            "export GH_REPO=$R; gh pr merge 5",
            "env GH_REPO=o/r gh pr merge 5",
            "env -u GH_TOKEN gh pr merge 5",
            "env --unset=GH_TOKEN gh pr merge 5",
            "unset GH_TOKEN; gh pr merge 5",
            "HOME=/tmp/h gh pr merge 5",
            "GIT_DIR=/tmp/g gh pr merge 5",
            "env -i gh pr merge 5",
        ] {
            assert!(
                only_merge(command, "/srv").unreadable.is_some(),
                "{command}"
            );
        }
        for command in [
            "GH_PROMPT_DISABLED=1 gh pr merge 5",
            "GH_PAGER=cat gh pr merge 5",
            "NO_COLOR=1 gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").unreadable, None, "{command}");
        }
    }

    /// `<(...)` and `>(...)` run their commands wherever they stand, as a
    /// word or as a redirection's file (ADR 0190).
    #[test]
    fn a_process_substitution_runs_its_commands() {
        for command in [
            "cat <(gh pr merge 5)",
            "cat < <(gh pr merge 5)",
            "tee >(gh pr merge 5)",
            "diff <(gh pr view 5) <(gh pr merge 5)",
            "echo 2>(gh pr merge 5) x",
            "echo a<(gh pr merge 5)",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        // Its commands are read with the command's, so a body given to
        // one that runs it is not data.
        assert!(words("cat <(sh) x").contains(&"sh".to_owned()));
        assert!(unclear("cat <<'EOF' > >(sh)\nit's a merge\nEOF"));
        assert!(!refused("diff <(git show HEAD:a) a"));
    }

    /// In `$(case ...)`, a pattern's `)` ends the pattern, not the
    /// substitution, until `esac` (ADR 0190).
    #[test]
    fn a_case_pattern_inside_a_substitution_does_not_end_it() {
        for command in [
            "echo \"$(case x in x) gh pr merge 5;; esac)\"",
            "echo $(case x in x) gh pr merge 5;; esac)",
            "echo $(case x in (x) gh pr merge 5;; esac)",
            "echo $(case x in y) :;; x) gh pr merge 5;; esac)",
            "echo $(case x in x) echo hi;; esac) && gh pr merge 5",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        assert!(!refused("echo \"$(case x in x) echo hi;; esac)\""));
    }

    /// Every redirection bash reads, with its file, is left out of the
    /// command's words (ADR 0190).
    #[test]
    fn every_redirection_bash_reads_leaves_the_command_s_own_words() {
        for command in [
            "gh pr &>/dev/null merge 5",
            "gh pr &>>/dev/null merge 5",
            "gh pr >| /dev/null merge 5",
            "gh pr 1>|/dev/null merge 5",
            "gh pr >&/dev/null merge 5",
            "gh pr >& /dev/null merge 5",
            "gh pr 2>&1 merge 5",
            "gh pr >&2 merge 5",
        ] {
            assert_eq!(words(command), ["gh", "pr", "merge", "5"], "{command}");
        }
    }

    /// In a command that mentions a merge, a program bash computes may be
    /// that merge (ADR 0190).
    #[test]
    fn a_program_bash_computes_in_a_command_that_mentions_a_merge_is_not_read() {
        for command in [
            "c='gh pr merge 5'; $c",
            "c='gh pr merge 5'; eval \"$c\"",
            "c='gh pr merge 5'; bash -c \"$c\"",
            "$(which gh) pr merge 5",
            "`which gh` pr merge 5",
            "\"$GH\" pr merge 5",
            // An unquoted body's substitutions run as it is read, data or
            // not.
            "cat <<EOF > notes.md\n$(c='gh pr merge 5'; $c)\nEOF",
        ] {
            assert!(unclear(command), "{command}");
        }
        // Without a merge in the text, nothing here is the gate's to read.
        for command in ["$EDITOR notes.md", "\"$HOME/.cargo/bin/cargo\" build"] {
            assert!(!refused(command), "{command}");
        }
    }

    /// A word bash computes may be any word gh or curl reads: a command, a
    /// flag, a method or an address (ADR 0190).
    #[test]
    fn a_word_bash_computes_that_may_decide_what_gh_or_curl_merges_leaves_it_unread() {
        for command in [
            "gh $(echo pr merge 5)",
            "gh pr $(echo merge) 5",
            "gh {pr,merge} 5",
            "gh pr {merge,} 5",
            "V=merge; gh pr ${V} 5",
            "V=merge; gh pr \"${V}\" 5",
            "for i in 5; do gh pr merge $i; done",
            "N=5; gh api -X PUT repos/o/r/pulls/$N/merge",
            "M=PUT; gh api -X $M repos/o/r/pulls/5/merge",
            "M='-X PUT'; gh api repos/o/r/pulls/5/merge $M",
            "Q='mutation { mergePullRequest(input:{}) { x } }'; gh api graphql -f query=\"$Q\"",
            "curl -X PUT -H x https://api.github.com/repos/o/r/pulls/$N/merge",
            "curl -X $M https://api.github.com/repos/o/r/pulls/5/merge",
        ] {
            assert!(unclear(command), "{command}");
        }
        // A pull request gh only views, and a header's value.
        for command in [
            "gh pr view $N --json mergeable,mergeStateStatus",
            "curl -H \"Authorization: Bearer $T\" https://api.github.com/repos/o/r/pulls/5 | jq .mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// A value bash reads as arithmetic or as a name may hold a subscript
    /// that runs commands, and a function runs in place of a program: in a
    /// command that mentions a merge, neither is read (ADR 0190).
    #[test]
    fn a_value_bash_evaluates_in_a_command_that_mentions_a_merge_is_not_read() {
        for command in [
            "x='a[$(gh pr merge 5)]'; echo $((x))",
            "x='a[$(gh pr merge 5)]'; (( x ))",
            "x='a[$(gh pr merge 5)]'; echo ${a[x]}",
            "x='a[$(gh pr merge 5)]'; echo ${!x}",
            "x='a[$(gh pr merge 5)]'; echo ${s:x}",
            "x='a[$(gh pr merge 5)]'; [[ $x -eq 1 ]]",
            "x='a[$(gh pr merge 5)]'; let x",
            "x='a[$(gh pr merge 5)]'; declare -i y=x",
            "x='a[$(gh pr merge 5)]'; a[x]=1",
            "x='a[$(gh pr merge 5)]'; printf -v 'a[x]' 1",
            "x='a[$(gh pr merge 5)]'; cat <<B\n$((x))\nB",
            // What an unquoted body runs as it is read: here git's config
            // runs a function.
            "cat <<B\n$(git -c core.fsmonitor='f() { gh pr \"$@\"; }; f merge 5' status)\nB",
            "f() { gh pr \"$@\"; }; f merge 5",
            "function f { gh pr \"$@\"; }; f merge 5",
            // bash reads a function by any name no assignment takes, as `=`,
            // or a quoted or computed one, and runs its body only when
            // called, here never.
            "=() { cd own; }; gh pr merge 5",
            "\"a\"=() { cd own; }; gh pr merge 5",
            "a$z=() { cd own; }; gh pr merge 5",
        ] {
            assert!(unclear(command), "{command}");
        }
        // A value read as it is: a name, a length, a default, a pattern.
        for command in [
            "ls ${HOME} && gh pr view 5 --json mergeable",
            "echo ${#} ${#x} ${x:-y} ${x%.md} ${x^^} && gh pr view 5 --json mergeable",
            "echo $((1+2)) && gh pr view 5 --json mergeable",
            "f() { echo hi; }; f",
            // An empty array is no function.
            "a=() && gh pr view 5 --json mergeable",
            // printf evaluates a subscript only in the name `-v` sets.
            "printf '%s %d\\n' 'a[1]' 'b[2]' && gh pr view 5 --json mergeable",
            // A quoted body is printed as written.
            "cat <<'B'\n$((x)) merge\nB",
            "cat <<'B'\n$(git -c core.fsmonitor='f() { gh pr \"$@\"; }; f merge 5' status)\nB",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// The mod sends the gate a command whose merge or gh is spelled with
    /// quotes or escapes, and the gate counts it as mentioning a merge
    /// (ADR 0190).
    #[test]
    fn a_merge_spelled_with_quotes_or_escapes_is_mentioned() {
        for command in [
            "gh pr mer''ge 5 && echo $((",
            "gh pr mer\\\nge 5 && echo $((",
            "gh pr mer\\ge 5 && echo $((",
            "gh pr $'mer\\x67e' 5 && echo $((",
            "gh pr mer$\"g\"e 5 && echo $((",
            "c$\"u\"rl -X PUT https://api.github.com/repos/o/r/pulls/5/merge && echo $((",
            "gh api graphql -f query='mutation { enqueuePullRequest(input:{}) { x } }' && echo $((",
        ] {
            assert!(unclear(command), "{command}");
        }
        assert!(!refused("gh pr view 5 && echo $(("));
        // `$"..."` is its text as a locale may translate it, which bash
        // computes, so a merge it spells is held, not read.
        assert!(unclear("gh pr mer$\"g\"e 5"));
    }

    /// Where a name may run something other than its program, no command
    /// is known to pass on the input it reads (ADR 0190).
    #[test]
    fn a_name_that_may_run_another_program_passes_on_no_input_the_gate_sees() {
        for before in [
            "PATH=/tmp/x:$PATH cat",
            "export PATH=/tmp/x:$PATH; cat",
            "hash -p /tmp/x cat; cat",
            "alias cat=/tmp/x; cat",
            ". /tmp/x; cat",
            "source /tmp/x; cat",
            "enable -n echo; cat",
        ] {
            let command = format!("{before} <<'C' | gh api graphql --input -\n{INNOCENT}\nC");
            assert!(
                only_merge(&command, "/srv").unreadable.is_some(),
                "{command}"
            );
        }
        for (definition, name) in [
            ("cat() { printf '%s' 'M'; }", "cat"),
            ("function cat { printf '%s' 'M'; }", "cat"),
            ("tee() { printf '%s' 'M'; }", "tee"),
        ] {
            let definition = definition.replace('M', MUTATION);
            let command =
                format!("{definition}; {name} <<'C' | gh api graphql --input -\n{INNOCENT}\nC");
            assert!(unclear(&command), "{command}");
        }
        // Without one, what cat passes on is seen.
        assert!(!refused(&format!(
            "cat <<'C' | gh api graphql --input -\n{INNOCENT}\nC"
        )));
    }

    /// curl's one-letter flags group, take their values attached or as the
    /// next word, and the data flags have other names (ADR 0190).
    #[test]
    fn curl_reads_its_flags_as_curl_does() {
        for command in [
            "curl -sX PUT https://api.github.com/repos/o/r/pulls/5/merge",
            "curl -XPUT https://api.github.com/repos/o/r/pulls/5/merge",
            "curl -sSfX PUT -H 'Accept: x' https://api.github.com/repos/o/r/pulls/5/merge",
            "curl -dq=1 -X PUT https://api.github.com/repos/o/r/pulls/5/merge",
            "curl --request PUT https://api.github.com/repos/o/r/pulls/5/merge",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        for command in [
            format!("curl -d'{MUTATION}' https://api.github.com/graphql"),
            format!("curl -sd '{MUTATION}' https://api.github.com/graphql"),
            format!("curl -sSd'{MUTATION}' https://api.github.com/graphql"),
            format!("curl --data-ascii '{MUTATION}' https://api.github.com/graphql"),
            format!("curl --data-urlencode '{MUTATION}' https://api.github.com/graphql"),
        ] {
            assert_eq!(
                only_merge(&command, "/srv").unreadable,
                Some("it names the pull request by a GraphQL id"),
                "{command}"
            );
        }
        // A flag's value is not an address.
        assert!(!refused(
            "curl -sS -w '%{http_code}' -o out.json https://api.github.com/repos/o/r/pulls/5/merge"
        ));
    }

    /// A body curl reads from a file, a variable or its own config, or an
    /// address it expands as a pattern, may be any merge (ADR 0190).
    #[test]
    fn a_curl_request_the_gate_cannot_see_is_held() {
        let unseen = unseen_request().unreadable;
        for command in [
            "curl -Kq.cfg",
            "curl -d@q.json https://api.github.com/graphql",
            "curl -sd @q.json https://api.github.com/graphql",
            "curl --data-ascii @q.json https://api.github.com/graphql",
            "curl --data-urlencode @q.json https://api.github.com/graphql",
            "curl --data-urlencode query@q.graphql https://api.github.com/graphql",
            "curl -T q.json -X POST https://api.github.com/graphql",
            "curl -F 'query=<q.graphql' https://api.github.com/graphql",
            "curl --variable %Q@q.json --expand-data '{{Q}}' https://api.github.com/graphql",
            "curl -d @q.json",
            "curl -d @q.json 'https://api.github.com/{graphql}'",
            "curl -d @q.json 'https://api.github.com/graph[p-q]l'",
            "printf 'url = x' > ~/.curlrc; curl -d @q.json",
            "printf 'url = x' > ~/.curlrc; curl",
        ] {
            assert_eq!(only_merge(command, "/srv").unreadable, unseen, "{command}");
        }
        assert!(
            only_merge(
                "curl -X PUT 'https://api.github.com/repos/o/r/pulls/{5}/merge'",
                "/srv"
            )
            .unreadable
            .is_some()
        );
        // -g reads an address as written, and -q first skips curl's config.
        for command in [
            "curl -g -d @q.json 'https://api.github.com/{graphql}'",
            "curl -g -X PUT 'https://api.github.com/repos/o/r/pulls/{5}/merge'",
            "printf 'url = x' > ~/.curlrc; curl -q -s https://api.github.com/repos/o/r/pulls/5",
        ] {
            assert!(!refused(command), "{command}");
        }
        // A variable alone changes nothing curl sends.
        assert!(!refused(&format!(
            "curl --variable %Q@q.json -d '{INNOCENT}' https://api.github.com/graphql"
        )));
    }

    /// GitHub reads a JSON escape in a request body as its letter (ADR
    /// 0190).
    #[test]
    fn a_mutation_spelled_with_a_json_escape_is_read() {
        let escaped = MUTATION.replace("mergePullRequest", "mer\\u0067ePullRequest");
        for command in [
            format!("curl -d '{escaped}' https://api.github.com/graphql"),
            format!("gh api graphql --input - <<'B'\n{escaped}\nB"),
        ] {
            assert_eq!(
                only_merge(&command, "/srv").unreadable,
                Some("it names the pull request by a GraphQL id"),
                "{command}"
            );
        }
    }

    /// A command set up to run in place of another, or later, is read where
    /// it is set: a git or gh alias, an editor, a shell alias, a trap
    /// (ADR 0190).
    #[test]
    fn a_command_set_up_to_run_in_place_of_another_is_read() {
        for command in [
            "git -c alias.m='!gh pr merge 5' m",
            "git -c core.editor='gh pr merge 5;:' commit --allow-empty",
            "git config alias.m '!gh pr merge 5'",
            "GIT_EDITOR='gh pr merge 5;:' git commit --allow-empty",
            "export GIT_EDITOR='gh pr merge 5;:'; git commit --allow-empty",
            "gh alias set m 'pr merge 5' && gh m",
            "gh alias set --shell m 'gh pr merge 5'; gh m",
            "gh alias set m '!gh pr merge 5'",
            "alias m='gh pr merge 5'; m",
            "trap 'gh pr merge 5' EXIT",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
    }

    /// `pushd` to one folder moves there; any other move the gate does not
    /// follow leaves the folder untold (ADR 0190).
    #[test]
    fn a_folder_change_the_gate_does_not_follow_leaves_the_folder_untold() {
        assert_eq!(
            only_merge("pushd /tmp && gh pr merge 5", "/srv")
                .folder
                .as_deref(),
            Some(Path::new("/tmp"))
        );
        for command in [
            "popd && gh pr merge 5",
            "pushd && gh pr merge 5",
            "pushd +1 && gh pr merge 5",
            "cd \"$D\" && gh pr merge 5",
            "env -C \"$D\" gh pr merge 5",
        ] {
            let merge = only_merge(command, "/srv");
            assert!(
                merge.folder.is_none() || merge.unreadable.is_some(),
                "{command}"
            );
        }
    }

    /// A shell, or `source`, that reads its script from a word bash
    /// computes, such as `<(...)`, or from input the gate cannot see may run
    /// any commands, so a command that mentions a merge is not read
    /// (ADR 0190).
    #[test]
    fn a_shell_reading_a_script_the_gate_cannot_see_is_not_read() {
        for command in [
            "bash <(echo gh pr merge 5)",
            "source <(echo gh pr merge 5)",
            ". <(echo gh pr merge 5)",
            "bash < <(echo gh pr merge 5)",
            "cat <(echo gh pr merge 5) | bash",
            "bash <(echo gh api -X PUT repos/o/r/pulls/5/merge)",
            "echo 'gh pr merge 5' | bash",
            "printf '%s\\n' 'gh pr merge 5' | sh -s",
            "bash -s < m.sh # gh pr merge 5",
            "bash -s arg # gh pr merge 5",
            "zsh \"$S\" # gh pr merge 5",
            "bash -o errexit \"$S\" # gh pr merge 5",
        ] {
            assert!(unclear(command), "{command}");
        }
        // The script's text the gate sees is read, and one that says no
        // merge is passed.
        assert_eq!(
            only_merge("bash <<'EOF'\ngh pr merge 5\nEOF", "/srv")
                .pull_request
                .as_deref(),
            Some("5")
        );
        for command in [
            "echo 'gh pr view 5' | bash",
            "bash <(echo ls) && gh pr view 5",
            "bash ./build.sh && gh pr view 5 --json mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// A `cd` in a command that may not run, or that runs in a shell of its
    /// own, leaves the folder of what follows untold, and so does one that
    /// `CDPATH` may send elsewhere (ADR 0190).
    #[test]
    fn a_folder_change_that_may_not_run_here_leaves_the_folder_untold() {
        for command in [
            "if false; then cd own; fi; gh pr merge 5",
            "if cd own; then :; fi; gh pr merge 5",
            "while false; do cd own; done; gh pr merge 5",
            "until true; do cd own; done; gh pr merge 5",
            "for d in; do cd own; done; gh pr merge 5",
            "if true; then :; else cd own; fi; gh pr merge 5",
            "case x in y) cd own;; esac; gh pr merge 5",
            "coproc { cd own; }; gh pr merge 5",
            "coproc { cd own; gh pr merge 5; }",
            "coproc NAME { cd own; gh pr merge 5; }",
            "{ cd own; } & gh pr merge 5",
            "cd own & gh pr merge 5",
            "(cd own; gh pr merge 5)",
            "false && cd own; gh pr merge 5",
            "true || cd own; gh pr merge 5",
            "cd own | cat; gh pr merge 5",
            "echo \"$(cd own)\"; gh pr merge 5",
            "eval 'cd own'; gh pr merge 5",
            "source ./env.sh; gh pr merge 5",
            "CDPATH=../x; cd own && gh pr merge 5",
            "export CDPATH=../x; cd own && gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").folder, None, "{command}");
        }
        // A `cd` that runs here, and one `CDPATH` does not search, is
        // followed, and a coprocess's moves nothing.
        for (command, folder) in [
            ("cd own && gh pr merge 5", "/srv/own"),
            ("cd own; gh pr merge 5", "/srv/own"),
            ("CDPATH=../x; cd ./own && gh pr merge 5", "/srv/own"),
            ("coproc cd own; gh pr merge 5", "/srv"),
            (
                "if true; then gh pr view 5; fi; cd own; gh pr merge 5",
                "/srv/own",
            ),
            ("case x in y) true; esac; cd own; gh pr merge 5", "/srv/own"),
            ("true && gh pr view 5; cd own; gh pr merge 5", "/srv/own"),
            ("cd own; sleep 1 & gh pr merge 5", "/srv/own"),
            ("cd own; eval 'echo hi'; gh pr merge 5", "/srv/own"),
            // The first command of an `&&` or `||` list runs.
            ("cd own || exit 1; gh pr merge 5", "/srv/own"),
        ] {
            assert_eq!(
                only_merge(command, "/srv").folder.as_deref(),
                Some(Path::new(folder)),
                "{command}"
            );
        }
        // A heredoc stays its command's input when the list it is in runs in
        // the background.
        assert!(!refused(
            "gh api graphql --input - <<'EOF' &\n{\"query\":\"query { x { mergeable } }\"}\nEOF"
        ));
        // So `bash` here still runs a body the gate cannot read.
        assert!(refused("bash <<'EOF' &\nIt's gh pr merge 5\nEOF"));
    }

    /// A `case` is read as bash reads it: its patterns are no command, a
    /// `)` ends a pattern wherever the `case` stands, and its clauses run
    /// commands, inside a substitution or not (ADR 0190).
    #[test]
    fn a_case_is_read_as_bash_reads_its_patterns_and_clauses() {
        for command in [
            "echo \"$( (case x in x) true;; esac) ; gh pr merge 5 )\"",
            "echo \"$(case x in x|esac) gh pr merge 5;; esac)\"",
            "echo \"$(time -p case x in x) gh pr merge 5;; esac)\"",
            "echo \"$(case x in (x) gh pr merge 5;; esac)\"",
            "echo \"$(case x in x) true;; y) gh pr merge 5;& esac)\"",
            "case x in x) gh pr merge 5;; esac",
            "case x in esac) true;; *) gh pr merge 5;; esac",
        ] {
            assert!(refused(command), "{command}");
        }
        for command in [
            "case x in merged) gh pr view 5;; esac",
            "echo \"$(case x in merged|y) gh pr view 5;; esac)\"",
        ] {
            assert!(!refused(command), "{command}");
        }
        // A `(` before a pattern opens nothing, and a pattern named as a
        // reserved word opens nothing, so a `cd` after the `case` is
        // followed.
        for command in [
            "case x in (y) true;; esac; cd own; gh pr merge 5",
            "case x in y) true;; if) true;; esac; cd own; gh pr merge 5",
        ] {
            assert_eq!(
                only_merge(command, "/srv").folder.as_deref(),
                Some(Path::new("/srv/own")),
                "{command}"
            );
        }
    }

    /// A reserved word is one only where bash reads a command. Given as an
    /// argument, as `echo if`, it is a word, and a `case` after it opens
    /// nothing, so the merge after it is read (ADR 0190).
    #[test]
    fn a_reserved_word_given_as_an_argument_opens_nothing() {
        for command in [
            "echo if case x in; gh pr merge 5",
            "echo do case x in; gh pr merge 5",
            ": then case x in\ngh pr merge 5",
            "echo time -p case x in; gh pr merge 5",
            "printf '%s\\n' if case x in; gh pr merge 5",
            "echo if case x in; gh api -X PUT repos/o/r/pulls/5/merge",
            "echo if case x in; curl -X PUT https://api.github.com/repos/o/r/pulls/5/merge",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        let graphql = "echo if case x in; gh api graphql -f query='mutation { mergePullRequest(input:{pullRequestId:\"X\"}) { clientMutationId } }'";
        assert!(only_merge(graphql, "/srv").unreadable.is_some());
        // Where bash does read a command, `case` still opens one.
        assert!(!refused("echo if; case x in merged) gh pr view 5;; esac"));
    }

    /// A closer given as an argument closes nothing, so a `cd` after it is
    /// still in the compound. Where bash reads a reserved word, after a
    /// compound's close, after `[[ ... ]]` and as `do` after `for` and its
    /// name, it is one, so the compound it opens is counted (ADR 0190).
    #[test]
    fn a_compound_is_counted_only_where_bash_reads_its_words() {
        for command in [
            "if true; then echo then fi; else cd own; fi; gh pr merge 5",
            "if false; then echo then fi; ! cd own; fi; gh pr merge 5",
            "if false; then echo then fi; cd own; fi; gh pr merge 5",
            "while false; do echo do done; cd own; done; gh pr merge 5",
            "if { false; } then if true; then :; fi; cd own; fi; gh pr merge 5",
            "if [[ -n x ]] then if true; then :; fi; cd own; fi; gh pr merge 5",
            "set --; for x do if true; then :; fi; cd own; done; gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").folder, None, "{command}");
        }
        // An opener given as an argument opens nothing, so the `cd` after
        // its line runs here.
        for command in [
            "echo then if; cd own; gh pr merge 5",
            "echo do {; cd own; gh pr merge 5",
        ] {
            assert_eq!(
                only_merge(command, "/srv").folder.as_deref(),
                Some(Path::new("/srv/own")),
                "{command}"
            );
        }
    }

    /// Text that ends inside a `case`, a compound command or `[[`, which
    /// bash refuses as a syntax error, is not read: one that mentions a
    /// merge is refused (ADR 0190).
    #[test]
    fn text_that_ends_inside_a_case_or_a_compound_is_not_read() {
        for command in [
            "echo hi; case x in\ngh pr merge 5",
            "case x in x) gh pr merge 5",
            "if true; then gh pr merge 5",
            "while true; do gh pr merge 5",
            "{ gh pr merge 5",
            "[[ -n x && gh == merge",
            "echo \"$(if true; then gh pr merge 5)\"",
        ] {
            assert!(unclear(command), "{command}");
        }
        for open in ["case x in", "if true; then echo hi", "{ echo", "[[ -n x"] {
            assert_eq!(tokens(open), None, "{open}");
        }
        for closed in [
            "if true; then echo hi; fi",
            "[[ -n x ]] && echo \"]]\"",
            "case x in x) echo;; esac",
        ] {
            assert!(tokens(closed).is_some(), "{closed}");
        }
    }

    /// Bash reads a command after `time`, `time -p`, `time --` and
    /// `time -p --`, so a reserved word there is one, and after `coproc`
    /// and a name a compound's opener is, `[[` among them. A word there
    /// that opens nothing is the name's argument (ADR 0190).
    #[test]
    fn time_s_options_and_a_coprocess_s_name_keep_a_command_position() {
        for command in [
            "time -- [[ x && case == in ]]; gh pr merge 5; case y in y) :;; esac",
            "time -p -- [[ x && case == in ]]; gh pr merge 5; case y in y) :;; esac",
            "coproc n [[ x && case == in ]]; gh pr merge 5; case y in y) :;; esac",
            "time -- [[ x && case == in ]]; gh api -X PUT repos/o/r/pulls/5/merge; case y in y) :;; esac",
            "time -- [[ x && case == in ]]; curl -X PUT https://api.github.com/repos/o/r/pulls/5/merge; case y in y) :;; esac",
            "time -- gh pr merge 5",
            "time -p -- gh pr merge 5",
            "coproc n time -p case x in; gh pr merge 5",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        let graphql = "time -- [[ x && case == in ]]; gh api graphql -f query='mutation { mergePullRequest(input:{pullRequestId:\"X\"}) { clientMutationId } }'; case y in y) :;; esac";
        assert!(only_merge(graphql, "/srv").unreadable.is_some());
        // The merge is gh's own, read as any other, not another program's.
        for command in ["time -- gh pr merge 5", "time -p -- gh pr merge 5"] {
            assert_eq!(only_merge(command, "/srv").unreadable, None, "{command}");
        }
        for command in [
            "time -- if false; then cd own; fi; gh pr merge 5",
            "time -p -- if false; then cd own; fi; gh pr merge 5",
            "time -- while false; do cd own; done; gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").folder, None, "{command}");
        }
        // After `time --` or `time -p`, a `-p` is a program's name, so an
        // `if` after it is its argument and opens nothing.
        for command in [
            "time -- -p if; gh pr merge 5",
            "time -p -p if; gh pr merge 5",
            "time -p -- -p if; gh pr merge 5",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        assert!(!refused("time -- cargo test merge_gate"));
    }

    /// An array's words are words, whatever they spell: a closer among
    /// them closes nothing, so a `cd` after it is still in the compound,
    /// and a substitution among them runs. A comment in one ends at the
    /// line's end, as bash reads it (ADR 0190).
    #[test]
    fn a_closer_in_an_array_closes_nothing() {
        for command in [
            "if false; then a=(fi); cd own; fi; gh pr merge 5",
            "while false; do a=(done); cd own; done; gh pr merge 5",
            "if false; then a=(}); cd own; fi; gh pr merge 5",
            "case x in y) a=(esac); cd own;; esac; gh pr merge 5",
            "if false; then a+=(fi); cd own; fi; gh pr merge 5",
            "if false; then a=(x\nfi); cd own; fi; gh pr merge 5",
            "if false; then a=(x # )\nfi); cd own; fi; gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").folder, None, "{command}");
        }
        assert_eq!(
            only_merge("a=(fi done); cd own; gh pr merge 5", "/srv")
                .folder
                .as_deref(),
            Some(Path::new("/srv/own"))
        );
        // `a[1]` is read as a glob, so the gate holds the command.
        assert!(refused(
            "if false; then a[1]=(fi); cd own; fi; gh pr merge 5"
        ));
        assert_eq!(words("a=(fi 'b' c) d"), ["a=(fi b c)", "d"]);
        assert_eq!(words("a=(x)y; a=(x\\\ny)"), ["a=(x)y", "<break>", "a=(xy)"]);
        // A `#` inside a word is the word's, a quoted `)` ends nothing, and
        // a newline is a space between words.
        assert_eq!(
            words("a=(x#y \"p )\" q) z; a=(p\nq)"),
            ["a=(x#y p ) q)", "z", "<break>", "a=(p q)"]
        );
        // A heredoc's body would start at a newline inside the array. Bash
        // 5.2.21 reads it after the array, to the end of the text; this does
        // not follow either reading.
        assert!(unclear("cat <<E; a=(x\nE\n)\ngh pr merge 5\nE"));
        for command in [
            "a=($(gh pr merge 5))",
            "a=(x \"$(gh pr merge 5)\")",
            "a=(`gh pr merge 5`)",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        assert_eq!(
            only_merge("a=(<(gh pr merge 5))", "/srv")
                .pull_request
                .as_deref(),
            Some("5")
        );
        // Bash refuses these as syntax errors.
        for broken in ["a=(x;y)", "a=( (x) )", "a=(x|y)"] {
            assert_eq!(tokens(broken), None, "{broken}");
        }
        // Only a name as written assigns an array.
        assert_eq!(words("\"a\"=(x) b"), ["a=", "<break>", "x", "<break>", "b"]);
        assert_eq!(
            words("a$z=(x) b"),
            ["<split:a$z=>", "<break>", "x", "<break>", "b"]
        );
    }

    /// Bash reads a `${...}` or `$[...]` whole, to the brace or bracket that
    /// matches its own, quotes, spaces and `;` among it, inside double
    /// quotes or not: a closer in one is no closer (ADR 0190).
    #[test]
    fn a_parameter_is_read_to_the_brace_that_matches_it() {
        for command in [
            "if false; then echo \"${x:-\"; fi; cd own; if :; then \"}\"; fi; gh pr merge 5",
            "if false; then echo \"${x:+\"; fi; cd own; if :; then \"}\"; fi; gh pr merge 5",
            "if false; then echo \"${x#\"; fi; cd own; if :; then \"}\"; fi; gh pr merge 5",
            "x=1; if false; then echo \"${x/\"; fi; cd own; if :; then \"/y}\"; fi; gh pr merge 5",
            "x=1; if false; then echo \"${x#'; fi; cd own; if :; then '}\"; fi; gh pr merge 5",
            "if false; then echo ${x:-; fi; cd own; if :; then }; fi; gh pr merge 5",
            "if false; then echo ${x:-'}'; fi; cd own; if :; then }; fi; gh pr merge 5",
        ] {
            assert_eq!(
                only_merge(command, "/srv").folder.as_deref(),
                Some(Path::new("/srv")),
                "{command}"
            );
        }
        // `$[...]` is arithmetic, which may run commands.
        for command in [
            "if false; then echo $[1; fi; cd own; if :; then 1]; fi; gh pr merge 5",
            "if false; then echo \"$[ \"1; fi; cd own; if :; then 1\" ]\"; fi; gh pr merge 5",
        ] {
            assert!(unclear(command), "{command}");
        }
        assert_eq!(
            only_merge(
                "if false; then : ${x:=a b}; fi; cd own; gh pr merge 5",
                "/srv"
            )
            .folder
            .as_deref(),
            Some(Path::new("/srv/own"))
        );
        assert_eq!(
            words("echo ${x:-a b} \"${y:-\"c d\"}\" $[1 + 2]"),
            [
                "echo",
                "<split:${x:-a b}>",
                "<printed:${y:-c d}>",
                "<opaque>",
                "<split:$[1 + 2]>"
            ]
        );
        // A substitution in one runs.
        assert_eq!(
            only_merge("echo \"${x:-$(gh pr merge 5)}\"", "/srv")
                .pull_request
                .as_deref(),
            Some("5")
        );
        assert_eq!(tokens("echo ${x:-a"), None);
        assert_eq!(
            words("echo \"$[ \"1 + 2\" ]\""),
            ["echo", "<opaque>", "<printed:$[ 1 + 2 ]>"]
        );
        // In double quotes nothing in one splits the word.
        assert_eq!(words("echo \"${w:-$z}\""), ["echo", "<printed:${w:-$z}>"]);
        // A backslash before a newline in one is taken out with it.
        assert_eq!(words("echo ${x:-a\\\nb}"), ["echo", "<split:${x:-ab}>"]);
        // Text nested deeper than `MAX_NESTING` is not read.
        assert!(tokens("echo ${a:-${b:-${c:-${d:-x}}}} ${e} ${f} ${g} ${h} ${i}").is_some());
        assert_eq!(tokens("echo ${a:-${b:-${c:-${d:-${e:-x}}}}}"), None);
    }

    /// In double quotes, the word of `${x-...}`, `${x+...}`, `${x=...}` or
    /// `${x?...}`, with a `:` or without, is double-quoted text: a `'` in
    /// it is a letter, so a substitution between two still runs, and in
    /// posix mode and in dash the first `}` ends the `${...}` even between
    /// them. Where a quote ends cannot be told, so a command that mentions
    /// a merge is not read past one. A pattern or a change of case keeps
    /// its quotes, and outside double quotes a quote is a quote (ADR 0190).
    #[test]
    fn a_quote_in_a_word_bash_expands_in_double_quotes_is_not_read_past() {
        for command in [
            "echo \"${x:-'$(gh pr merge 5)'}\"",
            "echo \"${x-'$(gh pr merge 5)'}\"",
            "x=1; echo \"${x:+'$(gh pr merge 5)'}\"",
            "x=1; echo \"${x+'$(gh pr merge 5)'}\"",
            "echo \"${x='$(gh pr merge 5)'}\"",
            "echo \"${x:='$(gh pr merge 5)'}\"",
            "echo \"${x?'$(gh pr merge 5)'}\"",
            "x=a; echo \"${x:?'$(gh pr merge 5)'}\"",
            "echo \"${x:-'`gh pr merge 5`'}\"",
            "echo \"${x:-'${y:-$(gh pr merge 5)}'}\"",
            "echo \"${x:-${y:-'$(gh pr merge 5)'}}\"",
            "echo \"${x:-$'$(gh pr merge 5)'}\"",
            "y=x; echo \"${!y:-'$(gh pr merge 5)'}\"",
            ": \"${x:-'$(gh pr merge 5)'}\"",
            "y=\"${x:-'$(gh pr merge 5)'}\"",
            "gh pr view 5 --json title -q \"${q:-'.title'}\" && echo \"${x:-'$(gh pr merge 5)'}\"",
            "cat <<E\n\"${x:-'$(gh pr merge 5)'}\"\nE",
            "echo \"${x:-'$(curl -X PUT https://api.github.com/repos/o/r/pulls/5/merge)'}\"",
            "echo \"${x:-'x' $(gh pr merge 5)}\"",
            "echo \"${x:-${y#a}'$(gh pr merge 5)'}\"",
            // Posix mode and dash end the `${...}` at the first `}`.
            "set -o posix\necho \"${x:-'}\"; gh pr merge 5; echo \"'}\"",
            "POSIXLY_CORRECT=1\necho \"${x:-'}\"; gh pr merge 5; echo \"'}\"",
            "x=a; set -o posix\necho \"${x?'}\"; gh pr merge 5; echo \"'}\"",
            "bash --posix <<'E'\necho \"${x:-'}\"; gh pr merge 5; echo \"'}\"\nE",
            "sh <<'E'\necho \"${x:-'}\"; gh pr merge 5; echo \"'}\"\nE",
            r#"dash -c "echo \"\${x:-'}\"; gh pr merge 5; echo \"'}\"""#,
            r#"sh -c "echo \"\${x:-'}\"; gh pr merge 5; echo \"'}\"""#,
        ] {
            assert!(refused(command), "{command}");
        }
        // Bash runs nothing quoted in a pattern or a change of case, nor
        // anything quoted outside double quotes.
        for command in [
            "x=a; echo \"${x#'$(gh pr merge 5)'}\"",
            "x=a; echo \"${x%'$(gh pr merge 5)'}\"",
            "x=a; echo \"${x/'$(gh pr merge 5)'/b}\"",
            "x=a; echo \"${x^'$(gh pr merge 5)'}\"",
            "x=a; echo \"${x,'$(gh pr merge 5)'}\"",
            "x=(a); echo \"${x[0]#'$(gh pr merge 5)'}\"",
            "echo \"${1%'$(gh pr merge 5)'}\"",
            "echo \"${$%'$(gh pr merge 5)'}\"",
            "x=a; echo \"${x:-${y#'$(gh pr merge 5)'}}\"",
            "x=a; echo \"${x#${y:-'$(gh pr merge 5)'}}\"",
            "echo ${x:-'$(gh pr merge 5)'}",
            "echo \"${x:-a}\" ${y:-'$(gh pr merge 5)'}",
            "cat <<'E'\n\"${x:-'$(gh pr merge 5)'}\"\nE",
        ] {
            assert_eq!(
                merges_in(command, Some(Path::new("/srv")), 0),
                Parsed::Merges(Vec::new()),
                "{command}"
            );
        }
    }

    /// Where bash reads an assignment, it reads a subscript after a name as
    /// written to the `]` that matches it, spaces, `;` and newlines among
    /// it, so a closer or a `case` in one is no word of its own. Bash reads
    /// one so at a command's start, after assignments, after redirections
    /// that begin the command, and at the start of an array's word. Such a
    /// subscript is arithmetic, so a command that mentions a merge is not
    /// read past one (ADR 0190).
    #[test]
    fn a_subscript_is_read_to_the_bracket_that_matches_it() {
        for command in [
            "if false; then a[ 0 ]=(fi); cd own; fi; gh pr merge 5",
            "if false; then a[ 0 ]+=(fi); cd own; fi; gh pr merge 5",
            "if false; then a[1 + 1]=(fi); cd own; fi; gh pr merge 5",
            "while false; do a[ 0 ]=(done); cd own; done; gh pr merge 5",
            "if false; then a[0; fi ]=1; cd own; fi; gh pr merge 5",
            "g=gh m=merge; false && a[0; case x in ]=1; $g pr $m 5; case y in y) :;; esac",
            "g=gh m=merge; false && a[ 0; case x in ]=1; $g pr $m 5; case y in y) :;; esac",
            "g=gh m=merge; false && b[0; case x in ]+=1; $g pr $m 5; case y in y) :;; esac",
            "if false; then x=1 a[0; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then x=\"1\" a[0; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then x=$(echo 1) a[0; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then >f a[0; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then 2>f x=1 a[0; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[0\nfi ]=1; cd own; fi; gh pr merge 5",
        ] {
            assert!(unclear(command), "{command}");
        }
        for command in [
            "if false; then a=([ x); fi ]=1); cd own; fi; gh pr merge 5",
            "if false; then a=(y [0; fi ]=1); cd own; fi; gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").folder, None, "{command}");
        }
        assert_eq!(
            words("a[ 0 ]=1; x=1 b[ 1 ]=2 c[2; 3 ]+=4"),
            [
                "<split:a[ 0 ]=1>",
                "<break>",
                "x=1",
                "<split:b[ 1 ]=2>",
                "<split:c[2; 3 ]+=4>"
            ]
        );
        assert_eq!(words(">f a[ 0 ]=1"), ["<split:a[ 0 ]=1>"]);
        // A quote in a subscript is the subscript's; a list or a pipe
        // begins a command.
        assert_eq!(
            words("b[\"1\"]=2 c[ 2 ]=3"),
            ["<split:b[1]=2>", "<split:c[ 2 ]=3>"]
        );
        assert_eq!(
            words("x=1 >f; a[ 0 ]=1 >g | b[ 1 ]=2"),
            [
                "x=1",
                "<break>",
                "<split:a[ 0 ]=1>",
                "<pipe>",
                "<split:b[ 1 ]=2>"
            ]
        );
        assert_eq!(words("a[ 0 ]x b[ 1 ]"), ["<split:a[ 0 ]x>", "b[", "1", "]"]);
        // Elsewhere bash splits it as any other word: after a command's
        // name, after a redirection that follows an assignment, and after
        // a word that only looks like an assignment.
        for (command, split) in [
            ("echo a[ 0 ]=1", vec!["echo", "a[", "0", "]=1"]),
            ("x=1 >f a[ 0 ]=1", vec!["x=1", "a[", "0", "]=1"]),
            (">f x=1 >g a[ 0 ]=1", vec!["x=1", "a[", "0", "]=1"]),
            ("\"x\"=1 a[ 0 ]=1", vec!["x=1", "a[", "0", "]=1"]),
            (
                "a[0]\"+\"=1 b[ 0 ]=1",
                vec!["<split:a[0]+=1>", "b[", "0", "]=1"],
            ),
            ("\"a\"[ 0 ]=1", vec!["a[", "0", "]=1"]),
            (
                "a[ 0 ]x=1 b[ 1 ]=2",
                vec!["<split:a[ 0 ]x=1>", "b[", "1", "]=2"],
            ),
            (
                "$(true)x=1 a[ 0 ]=1",
                vec![
                    "<split:x=1>",
                    "a[",
                    "0",
                    "]=1",
                    "<break>",
                    "true",
                    "<break>",
                ],
            ),
            (">a[ 0 ] b", vec!["0", "]", "b"]),
            // After a word bash computes a part of, in a case pattern, and
            // inside `[[ ... ]]` after `&&`: bash 5.2.21 runs `a[`, matches
            // `a[` and tests `a[` there.
            (
                "$(true)a[ 0 ]=1",
                vec!["<split:a[>", "0", "]=1", "<break>", "true", "<break>"],
            ),
            (
                "case x in y) :;; a[ ) echo two;; esac",
                vec![
                    "case", "x", "in", "<break>", ":", "<break>", "<break>", "echo", "two",
                    "<break>", "esac",
                ],
            ),
            (
                "[[ -n x && a[ ]] && echo t",
                vec![
                    "[[", "-n", "x", "<break>", "a[", "]]", "<break>", "echo", "t",
                ],
            ),
        ] {
            assert_eq!(words(command), split, "{command}");
        }
    }

    /// Inside a subscript bash reads a `[` to the `]` that matches it, a
    /// quote, a backslash and the character after it, a substitution and a
    /// parameter whole, so a `]` in one of them ends nothing (ADR 0190).
    #[test]
    fn a_bracket_quote_or_substitution_in_a_subscript_ends_nothing() {
        for command in [
            "if false; then a[b[0]; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[\"]\"; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[']'; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[\\]; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[${x:-]}; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[$(echo ]); fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[`echo ]`; fi ]=1; cd own; fi; gh pr merge 5",
            "if false; then a[$'\\x5d'; fi ]=1; cd own; fi; gh pr merge 5",
        ] {
            assert!(unclear(command), "{command}");
        }
        let substituted = vec![
            "<split:a[  ]=1>",
            "<break>",
            "echo",
            "]",
            "<break>",
            "echo",
            "0",
            "<break>",
        ];
        for (command, read) in [
            ("a[b[ 0 ] ]=1", vec!["<split:a[b[ 0 ] ]=1>"]),
            ("a[' ]' ]=1", vec!["<split:a[ ] ]=1>"]),
            ("a[\\ \\] ]=1", vec!["<split:a[ ] ]=1>"]),
            ("a[ `echo ] >/dev/null; echo 0` ]=1", substituted.clone()),
            ("a[ $(echo ] >/dev/null; echo 0) ]=1", substituted),
            ("a[ ${y:+]} 0 ]=1", vec!["<split:a[ ${y:+]} 0 ]=1>"]),
        ] {
            assert_eq!(words(command), read, "{command}");
        }
    }

    /// A word read as a `case` pattern runs nothing. One that names gh or
    /// curl, or spells a merge, may be a command read wrongly, so in a
    /// command that mentions a merge it is not read (ADR 0190).
    #[test]
    fn a_pattern_that_names_gh_or_a_merge_is_not_read() {
        for command in [
            "case x in gh\\ pr\\ merge\\ 5) :;; esac",
            "case $t in gh) :;; esac; gh pr view 5 --json mergeable",
            "case $t in curl) :;; esac; gh pr view 5 --json mergeable",
            "case $t in merge) :;; esac; gh pr view 5 --json state",
        ] {
            assert!(unclear(command), "{command}");
        }
        // A word that holds one of them is no word of one.
        for command in [
            "case $(gh pr view 5 --json state -q .state) in MERGED) echo done;; esac",
            "case $t in merged|ghost) :;; esac; gh pr view 5 --json mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// busybox and toybox run the applet named after them as that program,
    /// so `busybox xargs gh pr` is xargs giving gh its words (ADR 0190).
    #[test]
    fn busybox_runs_its_applet() {
        for command in [
            "echo merge 5 | busybox xargs gh pr",
            "echo merge 5 | toybox xargs gh pr",
            "echo merge 5 | /usr/bin/busybox xargs gh pr",
        ] {
            assert!(unclear(command), "{command}");
        }
        assert_eq!(
            only_merge("busybox sh -c 'gh pr merge 5'", "/srv")
                .pull_request
                .as_deref(),
            Some("5")
        );
        // A wrapper applet's merge is read as the merge, not held as one
        // another program runs.
        for command in [
            "busybox timeout 5 gh pr merge 5",
            "toybox timeout 5 gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").unreadable, None, "{command}");
        }
        for command in [
            "busybox --list",
            "echo 5 | busybox xargs gh pr view --json mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// `$'...'` is the text bash makes of it, each escape read, up to a
    /// `\0`, and not a word bash computes. So a merge after a tab or a
    /// newline in one is read, and the grant mod's spelling finds its gh
    /// (ADR 0190).
    #[test]
    fn an_ansi_c_string_is_the_text_bash_makes_of_it() {
        assert_eq!(
            words("echo $'a\\tb' $'\\x41\\101\\u0042' $'a\\0b' $'\\cA\\e' $'\\q' $'\\'' x"),
            ["echo", "a\tb", "AAB", "a", "\u{1}\u{1b}", "\\q", "'", "x"]
        );
        // Bash finds where the string ends first, reading a backslash and
        // the character after it as a pair, and then reads its escapes:
        // `\c` before a backslash takes a second one with it, and `\c` at
        // the end is kept as written.
        assert_eq!(
            words("echo $'\\c\\\\' $'a\\c\\\\b' $'\\c\\'x' $'a\\c' y"),
            ["echo", "\u{1c}", "a\u{1c}b", "\u{1c}'x", "a\\c", "y"]
        );
        assert_eq!(
            words("echo $'\\c\\'; gh pr merge 5 #'"),
            ["echo", "\u{1c}'; gh pr merge 5 #"]
        );
        for command in [
            "bash -c $'\\tgh pr merge 5'",
            "eval $'\\ngh pr merge 5'",
            "bash -c $'\\x09gh pr merge 5'",
            "gh pr $'merge' 5",
            "gh pr $'\\x6derge' 5",
            "echo $'\\c\\\\'; gh pr merge 5 #'",
            "echo $'\\c'; gh pr merge 5 #'",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        assert!(may_merge("bash -c $'\\tgh pr merge 5'"));
        assert!(may_merge("eval $'\\ngh pr merge 5'"));
        // Bash keeps an octal escape's low byte, so `\547` is `g`.
        assert_eq!(
            words("echo $'\\547' $'\\555' $'\\cI'"),
            ["echo", "g", "m", "\t"]
        );
        // A merge is mentioned as bash spells it: an escape outside
        // `$'...'` is its letter, and one inside is what bash makes of it.
        for command in [
            "gh pr $'\\x6d'\\erge 5",
            "gh pr mer$'\\x67'\\e 5",
            "gh pr $'\\x6d'e\\rge 5",
            "gh api -X PUT repos/o/r/pulls/5/$'\\x6d'\\erge",
            "$'\\x63'u\\rl -X PUT https://api.github.com/repos/o/r/pulls/5/merge",
            "gh pr mer$'\\547'e 5",
            "printf 'gh pr mer\\547e 5' | bash",
            "bash -c $'\\cIgh pr merge 5'",
            "eval $'\\cjgh pr merge 5'",
            "bash -c $'\\t'cu\\rl\\ -X\\ PUT\\ https://api.github.com/repos/o/r/pulls/5/merge",
            "printf '\\tgh pr merge 5' | bash",
            // One for each spelling, which only that spelling finds: read
            // as bash reads it, with its `%HH` read or without what it
            // expands, and with every escape read, the same two ways.
            "curl -X PUT https://api.github.com/repos/o/r/pulls/5/%6Derg\\e",
            "gh pr me${x}rg\\e 5",
            "printf 'curl -X PUT https://api.github.com/repos/o/r/pulls/5/%%6D\\x65rge' | bash",
            "printf '\\tg${x}h pr merge 5' | bash",
            // A `$'` that begins no string, inside quotes or after a
            // backslash, throws nothing after it off.
            "grep -c 'x$' notes.md; gh pr $'\\x6d'\\erge 5",
            "echo \"$'\"; gh pr mer$'\\x67'\\e 5",
            "echo \\$'; $'\\x63'u\\rl -X PUT https://api.github.com/repos/o/r/pulls/5/merge",
            "grep -c 'x$' notes.md; eval $'\\cjgh pr merge 5'",
            "grep -c 'x$' notes.md; bash -c $'\\t'cu\\rl\\ -X\\ PUT\\ https://api.github.com/repos/o/r/pulls/5/merge",
            // And one inside the merge's own word, where a reading of bash's
            // strings from either `$'` reads the other wrongly: a number's
            // escapes read and a letter's kept find it.
            "gh pr $'\\x6d'$(: \"$'\")\\erge 5 --repo 'o/r'",
            "gh pr $'\\x6d'$(: \"$'\")e\\rge 5 --repo 'o/r'",
            "gh pr mer$'\\x67'${z:+\"$'\"}\\e 5 --repo 'o/r'",
            "gh pr $'\\x6d'`: \"$'\"`\\erge 5 --repo 'o/r'",
            "gh api -X PUT repos/o/r/pulls/5/$'\\x6d'$(: \"$'\")\\erge -f 'a=b'",
            "$'\\x63'$(: \"$'\")u\\rl -X PUT https://api.github.com/repos/o/r/pulls/5/merge -H 'a: b'",
            "gh pr $(: \"$'\")$'\\x6d'$(: \"$'\")\\erge 5 --repo 'o/r'",
            "gh pr $'\\x6d'$(: '$')\\erge 5 --repo 'o/r'",
            // A `$'...'` string may make the tab or newline before gh or
            // curl, which bash splits a word at, so in a command with one,
            // gh or curl standing anywhere is enough.
            "eval $'\\cj'cu$(: \"$'\")\\rl' -X PUT https://api.github.com/repos/o/r/pulls/5/merge'",
            "bash -c $'\\t'cu$(: \"$'\")\\rl$'\\cI'-X\\ PUT\\ https://api.github.com/repos/o/r/pulls/5/merge",
            "eval $'\\cj'g${z:+\"$'\"}h' pr m'\\erge' 5'",
        ] {
            assert!(may_merge(command), "{command}");
        }
        // So a merge bash computes from such a string is held.
        assert!(unclear(
            "grep -c 'x$' notes.md; x=$'\\x6d'\\erge; gh pr $x 5"
        ));
        assert!(unclear("gh pr mer$'\\x67'${z:+\"$'\"}\\e 5 --repo 'o/r'"));
        // Without a `$'`, gh and curl count only as words of their own.
        assert!(!may_merge("git log --grep=merge --author=hugh"));
        assert!(may_merge(
            "printf $'\\n'; git log --grep=merge --author=hugh"
        ));
    }

    /// A program the gate does not know, given a word that spells a merge
    /// and gh or curl, may run it, as sed's `e`, expect's `spawn`, vim's
    /// `!`, `sg -c`, tmux and git's `rebase -x` do, and so may it what a
    /// command before it in the pipe prints, as m4, tclsh and `make -f -`
    /// do. Such a command is not read. A here-string bash computes no part
    /// of is input the gate sees, read as commands as a heredoc is, and a
    /// merge found there is held as another program's. grep and the
    /// commands the gate knows run nothing they are given (ADR 0190).
    #[test]
    fn a_program_given_a_merge_it_may_run_is_not_read() {
        for command in [
            "sed -n '1e gh pr merge 5' <<< x",
            "sed 's/.*/gh pr merge 5/e' <<< x",
            "echo x | sed -n '1e gh pr merge 5'",
            "echo 'syscmd(gh pr merge 5)' | m4",
            "echo 'exec gh pr merge 5' | tclsh",
            "printf 'all:\\n\\tgh pr merge 5\\n' | make -f -",
            "echo 'gh pr merge 5' | sudo bash",
            "echo 'gh pr merge 5' | ssh host",
            "echo gh pr merge 5 | m4",
            // A merge a variable holds, printed into such a program.
            "c='gh pr merge 5'; echo \"$c\" | sudo bash",
            "c='gh pr merge 5'; echo \"$c\" | ssh host",
            "c='gh pr merge 5'; sudo bash <<< \"$c\"",
            "c='syscmd(gh pr merge 5)'; printf '%s' \"$c\" | m4",
            "c='exec gh pr merge 5'; tclsh <<< \"$c\"",
            "expect -c 'spawn gh pr merge 5; expect eof'",
            "vim -es -u NONE -c '!gh pr merge 5' -c q",
            "sg users -c 'gh pr merge 5'",
            "tmux new -d 'gh pr merge 5'",
            "sudo sed -n '1e gh pr merge 5' <<< x",
            "git rebase -x 'gh pr merge 5' HEAD~1",
            "git -C . rebase --exec='gh pr merge 5' HEAD~1",
            "git submodule foreach 'gh pr merge 5'",
        ] {
            assert!(unclear(command), "{command}");
        }
        for command in [
            "m4 <<< 'syscmd(gh pr merge 5)'",
            "tclsh <<< 'exec gh pr merge 5'",
            "make -f - <<< $'all:\\n\\tgh pr merge 5'",
        ] {
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.pull_request.as_deref(), Some("5"), "{command}");
            assert_eq!(merge.unreadable, Some(THROUGH_ANOTHER_PROGRAM), "{command}");
        }
        // A here-string bash computes in part is input the gate cannot see.
        assert_eq!(
            words("m4 <<< \"$x\" <<< 'syscmd(gh)'"),
            ["m4", "<input>", "<stdin:syscmd(gh)>"]
        );
        // git's own command that runs its words runs them as another
        // program's.
        assert_eq!(
            only_merge("git bisect run gh pr merge 5", "/srv").unreadable,
            Some(THROUGH_ANOTHER_PROGRAM)
        );
        for command in [
            "grep -rn 'gh pr merge' docs",
            "git log --grep='gh pr merge'",
            "git commit -m 'Say gh pr merge 5 needs --repo'",
            "echo 'gh pr merge 5' >> notes.md",
            "gh pr comment 5 --body 'Run gh pr merge 5 once it is green'",
            "gh pr view \"$N\" --json mergeable | jq -r .mergeable",
            "gh pr view 5 --json mergeable,mergeStateStatus | jq -r .mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// xargs gives its command words from its input, which the gate does
    /// not see: words bash computes, at the end or, with `-I`, where its
    /// string stands. A string it gives a shell may be any commands
    /// (ADR 0190).
    #[test]
    fn xargs_gives_its_command_words_the_gate_cannot_see() {
        for command in [
            "echo merge 5 | xargs gh pr",
            "printf merge | xargs -I{} gh pr {} 5",
            "printf merge | xargs -I {} gh pr {} 5",
            "printf merge | xargs --replace=R gh pr R 5",
            "echo 'gh pr merge 5' | xargs -I{} bash -c {}",
            "echo 'gh pr merge 5' | xargs -i sh -c {}",
            "echo 'gh pr merge 5' | xargs bash -c",
            "echo 3 | xargs -n 1 sudo gh pr merge",
            // An option this does not know may change what it reads.
            "printf merge | xargs -J % gh pr % 5",
        ] {
            assert!(unclear(command), "{command}");
        }
        assert_eq!(
            only_merge("echo x | xargs -0 bash -c 'gh pr merge 5'", "/srv")
                .pull_request
                .as_deref(),
            Some("5")
        );
        for command in [
            "gh pr list --json number --jq '.[].number' | xargs -n1 gh pr view --json mergeable",
            "echo 5 | xargs -I{} gh pr view {} --json mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// A program this does not know, as ionice, taskset, chrt, flock,
    /// setsid or sudo, may run a command among its words. What that command
    /// leaves in doubt, as the words xargs gives it or a subcommand bash
    /// computes, is in doubt for the whole (ADR 0190).
    #[test]
    fn a_command_another_program_runs_keeps_its_doubt() {
        for command in [
            "echo merge 5 | ionice -c 3 xargs gh pr",
            "echo merge 5 | ionice xargs gh pr",
            "echo merge 5 | ionice -c 3 env xargs gh pr",
            "echo merge 5 | nice ionice xargs gh pr",
            "echo merge 5 | taskset -c 0 xargs gh pr",
            "echo merge 5 | taskset 1 xargs gh pr",
            "echo merge 5 | chrt -o 0 xargs gh pr",
            "echo merge 5 | flock f xargs gh pr",
            "echo merge 5 | flock -n f xargs gh pr",
            "echo merge 5 | setsid -w xargs gh pr",
            "echo merge 5 | setsid xargs gh pr",
            "echo merge 5 | sudo xargs gh pr",
            "ionice -c 3 xargs gh pr <<< 'merge 5'",
            "printf merge | ionice xargs -I{} gh pr {} 5",
            "m=merge; ionice -c 3 gh pr $m 5",
            "m=merge; taskset 1 gh pr $m 5",
            "c='gh pr merge 5'; ionice -c 3 sh -c \"$c\"",
        ] {
            assert!(unclear(command), "{command}");
        }
        for command in [
            "gh pr list --json number --jq '.[].number' | ionice xargs -n1 gh pr view --json mergeable",
            "echo 5 | taskset 1 xargs -I{} gh pr view {} --json mergeable",
            "ionice -c 3 gh pr view 5 --json mergeable",
            // A word bash computes among its words is no command of its own.
            "jq -r $f out.json && gh pr view 5 --json mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// Text a shell parses as commands, given in a word bash computes in
    /// part, is parsed with what bash put there, so `bash -c "... $N"`,
    /// `eval`, a runner's string, `trap`, an alias and the values that name
    /// a program may run any commands: a command that mentions a merge is
    /// not read (ADR 0190).
    #[test]
    fn script_text_bash_computes_may_be_any_commands() {
        for run in [
            "bash -c \"gh pr view $N\"",
            "bash -c \"gh pr view $(cat n)\"",
            "eval \"gh pr view $N\"",
            "eval gh pr view \"$N\"",
            "su w -c \"gh pr view $N\"",
            "script -qc \"gh pr view $N\" /dev/null",
            "ssh host \"gh pr view $N\"",
            "watch \"gh pr view $N\"",
            "trap \"gh pr view $N\" EXIT",
            "alias v=\"gh pr view $N\"",
            "env -S \"gh pr view $N\"",
            "git -c alias.v=\"!gh pr view $N\" v",
            "gh alias set v --shell \"gh pr view $N\"",
            "GIT_EDITOR=\"gh pr view $N\" git commit",
            "export GIT_EDITOR=\"gh pr view $N\"",
        ] {
            let command = format!("N='5; gh pr merge 5'; {run}");
            assert!(unclear(&command), "{command}");
        }
        for command in [
            "bash -c 'gh pr view \"$1\" --json mergeable' _ 5",
            "eval 'gh pr view 5 --json mergeable'",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// `cd` moves as bash's does: past its flags, in a group or not, and
    /// `--`, to its one folder. Given more than one, or a flag it does not
    /// have, it refuses and moves nowhere, which the gate does not follow
    /// (ADR 0190).
    #[test]
    fn cd_moves_only_to_one_folder_past_its_flags() {
        for command in ["cd own x; gh pr merge 5", "cd -x own; gh pr merge 5"] {
            assert_eq!(only_merge(command, "/srv").folder, None, "{command}");
        }
        for (command, folder) in [
            ("cd -- own; gh pr merge 5", "/srv/own"),
            ("cd -LP own; gh pr merge 5", "/srv/own"),
            ("cd -P -- own; gh pr merge 5", "/srv/own"),
            ("cd -- -x; gh pr merge 5", "/srv/-x"),
        ] {
            assert_eq!(
                only_merge(command, "/srv").folder.as_deref(),
                Some(Path::new(folder)),
                "{command}"
            );
        }
    }

    /// A command may set a variable whose name bash computes, as
    /// `declare "CD${z}PATH=..."` and `printf -v "$n"` do. It may be any
    /// name: `CDPATH`, `PATH` and the variables gh reads among them. A
    /// shell the command runs inherits a `CDPATH` set around it
    /// (ADR 0190).
    #[test]
    fn a_name_bash_computes_may_be_any_name() {
        for command in [
            "export CDPATH=../x; bash -c 'cd own && gh pr merge 5'",
            "export CD${z}PATH=../x; eval 'cd own && gh pr merge 5'",
            "declare \"CD${z}PATH=../x\"; cd own && gh pr merge 5",
            "export CD${z}PATH=../x; cd own && gh pr merge 5",
            "printf -v CD${z}PATH ../x; cd own && gh pr merge 5",
            "n=CD; declare \"${n}PATH=../x\"; cd own && gh pr merge 5",
            "read -r \"$n\" <<< ../x; cd own && gh pr merge 5",
            "declare -n r=\"$n\"; r=../x; cd own && gh pr merge 5",
        ] {
            assert_eq!(only_merge(command, "/srv").folder, None, "{command}");
        }
        assert_eq!(
            only_merge("export GH_${z}REPO=o/r; gh pr merge 5", "/srv").unreadable,
            Some(SETS_WHAT_GH_READS)
        );
        assert!(refused(&format!(
            "export PA${{z}}TH=/tmp/x; cat <<'C' | gh api graphql --input -\n{INNOCENT}\nC"
        )));
        // A value bash computes, under a name as written, sets that name.
        assert_eq!(
            only_merge("export FOO=\"$x\"; cd own && gh pr merge 5", "/srv")
                .folder
                .as_deref(),
            Some(Path::new("/srv/own"))
        );
    }

    /// An address's path is read with each `%HH` escape decoded, as a
    /// server may read it. Whether GitHub's does was not tried (ADR 0190).
    #[test]
    fn an_address_is_read_with_its_percent_escapes_decoded() {
        for command in [
            "curl -X PUT https://api.github.com/repos/o/r/pulls/5/%6derge",
            "gh api -X PUT repos/o/r/pulls/5/%6Derge",
            "gh api -X PUT repos/o/r/pulls/5%2Fmerge",
        ] {
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.pull_request.as_deref(), Some("5"), "{command}");
            assert_eq!(merge.repo.as_deref(), Some("o/r"), "{command}");
        }
        assert!(mentions_a_merge("gh api -X PUT repos/o/r/pulls/5/%6derge"));
        // Bash takes out a quote or a backslash inside one before the
        // server reads the address, outside `$'...'` a backslash and a
        // digit are that digit, and a `$'...'` string may make a digit.
        for command in [
            "curl -X PUT https://api.github.com/repos/o/r/pulls/5/%6''derge",
            "curl -X PUT 'https://api.github.com/repos/o/r/pulls/5/%6'\"d\"erge",
            "gh api -X PUT repos/o/r/pulls/5/%6\\Derge",
            "curl -X PUT https://api.github.com/repos/o/r/pulls/5/m%6\\5rge",
            "curl -X PUT https://api.github.com/repos/o/r/pulls/5/%$'\\x36'derge",
        ] {
            assert!(may_merge(command), "{command}");
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.pull_request.as_deref(), Some("5"), "{command}");
        }
        assert!(refused(&format!(
            "gh api graph%71l --input - <<'EOF'\n{MUTATION}\nEOF"
        )));
    }

    /// A runner given commands as one string, as `script -qc`, `flock -c`,
    /// `su -c`, a shell's `-c`, `ssh` and `watch` are, runs them in a
    /// shell, so they are read, and held as another program's (ADR 0190).
    #[test]
    fn a_runner_given_commands_as_a_string_is_read() {
        for command in [
            "script -qc 'gh pr merge 5' /dev/null",
            "script -c 'gh pr merge 5'",
            "flock /tmp/l -c 'gh pr merge 5'",
            "su w -c 'gh pr merge 5'",
            "su --command='gh pr merge 5' w",
            "runuser -l w -c 'gh pr merge 5'",
            "su -c'gh pr merge 5' w",
            "rbash -c 'gh pr merge 5'",
            "ksh -c 'gh pr merge 5'",
            "ssh -p 22 host 'gh pr merge 5'",
            "ssh host gh pr merge 5",
            // A host is no command, here one an ssh config may name `echo`,
            // and `-l` takes the next word.
            "ssh echo 'gh pr merge 5'",
            "ssh -l w echo 'gh pr merge 5'",
            // A flag that takes a value takes the next word, even one that
            // looks like `-c`.
            "su -g -c -c 'gh pr merge 5' w",
            "runuser -u -c -c 'gh pr merge 5'",
            "flock -E -c -c 'gh pr merge 5' /tmp/l",
            "script -O -c -c 'gh pr merge 5'",
            "watch -n 5 'gh pr merge 5'",
            "watch --interval 5 'gh pr merge 5'",
            "env -vS'gh pr merge 5'",
            "env -iS'gh pr merge 5'",
            "env -0vS 'gh pr merge 5'",
        ] {
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.pull_request.as_deref(), Some("5"), "{command}");
        }
        for command in [
            "script -qc 'gh pr view 5' /dev/null",
            "ssh host gh pr view 5",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// Code in another language may run gh or curl, so a program that runs
    /// it, given in a word that spells a merge and gh or curl, or read from
    /// a word bash computes or from input the gate cannot see, leaves a
    /// command that mentions a merge unread (ADR 0190).
    #[test]
    fn a_program_in_another_language_that_may_merge_is_not_read() {
        for command in [
            "python3 -c 'import os; os.system(\"gh pr merge 5\")'",
            "python -c \"__import__('os').system('gh pr merge ' + n)\"",
            "perl -e 'system(\"gh pr merge 5\")'",
            "ruby -e '`gh pr merge 5`'",
            "node -e 'require(\"child_process\").execSync(\"gh pr merge 5\")'",
            "awk 'BEGIN { system(\"gh pr merge 5\") }'",
            "python3 \"$S\" # gh pr merge 5",
            "python3 < m.py # gh pr merge 5",
            "python3 -q && gh pr view 5 --json mergeable",
            // Code it reads on standard input, as it is given there.
            "python3 <<'EOF'\nimport os\nos.system(\"gh pr merge 5\")\nEOF",
            "perl - <<'EOF'\nsystem(\"gh pr merge 5\");\nEOF",
            "cat <<'EOF' | node\nrequire(\"child_process\").execSync(\"gh pr merge 5\")\nEOF",
        ] {
            assert!(unclear(command), "{command}");
        }
        for command in [
            "python3 -c 'print(\"merge\")' && gh pr view 5",
            "python3 -c 'print(\"high merge\")' && gh pr view 5",
            "python3 -m pytest -k merge && gh pr view 5",
            "node build.js && gh pr view 5 --json mergeable",
            // These print about the program and exit, reading no code.
            "python3 --version && gh pr view 5 --json mergeable",
            "node --help && gh pr view 5 --json mergeable",
        ] {
            assert!(!refused(command), "{command}");
        }
    }

    /// `-T` sends a file with PUT, and curl takes any start of a long
    /// flag's name that only one of its flags begins with (ADR 0190).
    #[test]
    fn curl_takes_an_upload_and_a_start_of_a_flag_as_curl_does() {
        for command in [
            "curl -T /dev/null https://api.github.com/repos/o/r/pulls/5/merge",
            "curl -sT /dev/null https://api.github.com/repos/o/r/pulls/5/merge",
            "curl --upload-file m.json https://api.github.com/repos/o/r/pulls/5/merge",
        ] {
            assert_eq!(
                only_merge(command, "/srv").pull_request.as_deref(),
                Some("5"),
                "{command}"
            );
        }
        for flag in [
            "--data-r",
            "--js",
            "--data-b",
            "--upload-f",
            "--conf",
            "--req",
        ] {
            let command = format!(
                "curl {flag} '{{\"query\":\"mutation {{ mergePullRequest(input: {{}}) {{ x }} }}\"}}' https://api.github.com/graphql"
            );
            assert!(refused(&command), "{command}");
        }
        assert!(!refused(
            "curl -T log.txt https://example.com/upload && gh pr view 5 --json mergeable"
        ));
        // `-X` decides the method, before `-T` or after; GitHub merges only
        // on PUT.
        for command in [
            "curl -X POST -T m.json https://api.github.com/repos/o/r/pulls/5/merge",
            "curl -T m.json -X POST https://api.github.com/repos/o/r/pulls/5/merge",
        ] {
            assert!(!refused(command), "{command}");
        }
        // A config may hold the whole request, whatever follows it.
        assert!(refused(
            "curl -K c.cfg -d @q.json https://example.com/x # merge"
        ));
        assert!(!refused(
            "curl --sil https://api.github.com/repos/o/r/pulls/5 | jq .mergeable"
        ));
        assert!(!refused(
            "curl -s --disable https://api.github.com/repos/o/r/pulls/5 | jq .mergeable"
        ));
    }

    /// In `gh api` and curl, an address bash computes as one word is read
    /// as any address: a request that may then merge is held, and one that
    /// cannot is passed, as are the values of flags that only shape what
    /// gh prints. A word that may split stays unread (ADR 0190).
    #[test]
    fn a_computed_address_of_one_word_is_read_as_any_address() {
        for command in [
            "gh api \"repos/$REPO/pulls/5\" --jq .merged",
            "gh api repos/o/r/pulls/\"$N\" --jq .mergeable",
            "gh api \"$URL\" -q .mergeable",
            "gh api \"$X\"/pulls/5 --jq .mergeable",
            "gh api \"repos/o/r/pulls/$N\" --jq \"$Q\" # merged",
            "gh api \"repos/o/r/pulls/$N\" --jq=\"$Q\" # merged",
            "curl -s \"https://api.github.com/repos/o/r/pulls/$N\" | jq .mergeable",
            "curl -s --url \"$U\" | jq .mergeable",
            "curl -H \"Authorization: token $T\" https://api.github.com/repos/o/r/pulls/5 | jq .mergeable",
            "prs=(5 6); for pr in \"${prs[@]}\"; do gh pr view \"$pr\" --json mergeable; done",
        ] {
            assert!(!refused(command), "{command}");
        }
        for command in [
            "gh api -X PUT \"repos/o/r/pulls/$N/merge\"",
            "gh api --method PUT \"$URL\"",
            "gh api \"$URL\" -f query='mutation { mergePullRequest(input: {}) { x } }'",
            "gh api \"$URL\" --input m.json # merge",
            "curl -X PUT \"https://api.github.com/$P\" # merge",
            "curl -X PUT --url \"$URL\" # merge",
            "curl -T m.json \"https://api.github.com/$P\" # merge",
            "curl -d @q.json \"https://api.github.com/$P\" # merge",
        ] {
            assert!(refused(command), "{command}");
            assert!(!unclear(command), "held, not unread: {command}");
        }
        // An address with a word bash computes is still read as written: a
        // brace list there that curl or bash may make the merge's is held.
        assert!(
            only_merge(
                "curl -X PUT https://api.github.com/repos/o/r/pulls/5/mer{ge,}",
                "/srv"
            )
            .unreadable
            .is_some()
        );
        // An address bash computes from its first letter may be a flag of
        // curl's, `-K` among them, which reads any request from a file.
        for command in [
            "gh api repos/$REPO/pulls/5 --jq .mergeable",
            "gh api \"$FLAG\" repos/o/r/pulls/5 --jq .mergeable",
            "curl -s https://api.github.com/repos/o/r/pulls/$N | jq .mergeable",
            "curl \"$FLAG\" https://api.github.com/repos/o/r/pulls/5 # merge",
            "curl -s \"$URL\" | jq .mergeable",
            "curl -X\"$M\" https://api.github.com/repos/o/r/pulls/5/merge",
            "curl --\"$X\" https://api.github.com/repos/o/r/pulls/5/merge -X PUT",
        ] {
            assert!(unclear(command), "{command}");
        }
    }

    /// The gate looks for a merge with what bash expands taken out of the
    /// text, as `${z}`, `$(...)`, backquotes and `$1` may be empty, so a
    /// merge spelled around one is mentioned (ADR 0190).
    #[test]
    fn a_merge_spelled_around_an_expansion_is_mentioned() {
        for command in [
            "gh pr mer${z}ge 5",
            "gh pr mer$1ge 5",
            "g${z}h pr merge 5",
            "gh pr mer$(true)ge 5 && echo $((",
            "gh pr mer`true`ge 5 && echo $((",
            "gh pr mer$z\"ge\" 5 && echo $((",
            "gh pr mer$( )ge 5 && echo $((",
        ] {
            assert!(unclear(command), "{command}");
        }
        assert!(!refused("gh pr view ${N} && echo $(("));
    }

    /// `PATH` named to a command that sets it, as `read` and `printf -v`
    /// are, or through a reference to it, may make a name run another
    /// program (ADR 0190).
    #[test]
    fn path_set_through_a_command_or_a_reference_redefines_names() {
        for before in [
            "read -r PATH <<< /tmp/x; cat",
            "declare -n p=PATH; p=/tmp/x; cat",
            "printf -v PATH %s /tmp/x; cat",
            "IFS=: read -ra PATH <<< /tmp/x; cat",
        ] {
            let command = format!("{before} <<'C' | gh api graphql --input -\n{INNOCENT}\nC");
            assert!(
                only_merge(&command, "/srv").unreadable.is_some() || unclear(&command),
                "{command}"
            );
        }
    }

    /// In double quotes, `"$@"` and `"${a[@]}"` are each any number of
    /// words, so in a merge they may be any flags; an array's element by
    /// number reads no value as arithmetic, and a count is one word
    /// (ADR 0190).
    #[test]
    fn an_array_or_the_positional_parameters_in_quotes_are_many_words() {
        for command in [
            "gh pr merge 5 --subject \"$@\"",
            "gh pr merge 5 --subject \"${@}\"",
            "gh pr merge 5 --subject \"${a[@]}\"",
            "gh pr merge 5 --body \"${!a[@]}\"",
        ] {
            assert!(unclear(command), "{command}");
        }
        for command in [
            "gh pr merge 5 --subject \"${a[2]}\" --match-head-commit abc1234",
            "gh pr merge 5 --subject \"$*\" --match-head-commit abc1234",
            "gh pr merge 5 --subject \"${#a[@]}\" --match-head-commit abc1234",
        ] {
            assert!(!unclear(command), "{command}");
        }
    }

    /// What bash makes of an unquoted heredoc holding `$`, a backquote or
    /// a backslash is not its text, so a request read from it is held as
    /// one the gate cannot see (ADR 0190).
    #[test]
    fn a_request_read_from_a_heredoc_bash_changes_is_held() {
        for command in [
            "cat <<B | gh api graphql --input -\n{\"query\": \"mutation { merge${x}PullRequest(input: {}) { x } }\"}\nB",
            "gh api graphql --input - <<B\n{\"query\": \"mutation { merge`printf ''`PullRequest(input: {}) { x } }\"}\nB",
            "curl -d @- https://api.github.com/graphql <<B\n{\"query\": \"mutation { merge\\\nPullRequest(input: {}) { x } }\"}\nB",
            // Bash makes `\\` one backslash, which JSON then reads with
            // the `u0067` after it as a `g`.
            "gh api graphql --input - <<B\n{\"query\": \"mutation { mer\\\\u0067ePullRequest(input: {}) { x } }\"}\nB",
        ] {
            let merge = only_merge(command, "/srv");
            assert_eq!(merge.head, Head::Unnamed, "{command}");
            assert!(merge.unreadable.is_some(), "{command}");
        }
        // Without them, or quoted, the body is what gh reads.
        for command in [
            format!("cat <<B | gh api graphql --input -\n{INNOCENT}\nB"),
            format!("gh api graphql --input - <<'B'\n{INNOCENT}\nB"),
            "gh api graphql --input - <<'B'\n{\"query\": \"query($n: Int!) { viewer { login } }\", \"variables\": {\"n\": 1}}\nB".to_owned(),
            "gh api graphql --input - <<'B'\n{\"query\": \"query { viewer { login } } # a\\\\b\"}\nB".to_owned(),
        ] {
            assert!(!refused(&command), "{command}");
        }
    }

    fn pull(author: &str, owner: &str, public: bool) -> Facts {
        Facts {
            merger: "miopea".into(),
            repo: format!("{owner}/swarm-next"),
            repo_owner: owner.into(),
            public,
            number: 7,
            author: author.into(),
            author_is_bot: author.starts_with("app/"),
            head: "35e32938aaaa".into(),
        }
    }

    fn read(head: Head, facts: Facts) -> Judged {
        Judged::Read { head, facts }
    }

    #[test]
    fn only_someone_elses_pull_request_in_the_operators_own_public_repository_is_covered() {
        let named = Head::Named("35e32938".into());
        for outside in [
            pull("miopea", "miopea", true),
            pull("MIOPEA", "miopea", true),
            pull("a-teammate", "another-owner", true),
            pull("app/dependabot", "another-owner", false),
            pull("a-friend", "miopea", false),
        ] {
            assert_eq!(
                judge(&[read(Head::Unnamed, outside.clone())]),
                Judgement::Pass,
                "{outside:?}"
            );
        }
        // A bot is never the operator, even one signed in under the same name.
        let same_name_bot = Facts {
            author_is_bot: true,
            ..pull("miopea", "miopea", true)
        };
        for covered in [
            pull("a-stranger", "miopea", true),
            pull("app/dependabot", "miopea", true),
            same_name_bot,
        ] {
            let Judgement::NeedsGrant(why) = judge(&[read(named.clone(), covered.clone())]) else {
                panic!("{covered:?} needs the operator's grant");
            };
            assert!(why.contains(&covered.author), "{why}");
            assert!(why.contains("not by miopea"), "{why}");
        }
    }

    #[test]
    fn a_covered_merge_must_name_the_head_it_merges_and_that_head_must_be_current() {
        let stranger = pull("a-stranger", "miopea", true);
        let Judgement::Refuse(why) = judge(&[read(Head::Unnamed, stranger.clone())]) else {
            panic!("a merge that names no head is refused outright");
        };
        assert!(why.contains("--match-head-commit"), "{why}");

        let Judgement::Refuse(why) =
            judge(&[read(Head::Named("0000000".into()), stranger.clone())])
        else {
            panic!("a merge of an old head is refused outright");
        };
        assert!(
            why.contains("Its head is now 35e32938aaaa, not 0000000"),
            "{why}"
        );

        assert!(matches!(
            judge(&[read(Head::Named("35E3293".into()), stranger.clone())]),
            Judgement::NeedsGrant(_)
        ));
        assert!(
            matches!(
                judge(&[read(Head::Named("35e3".into()), stranger.clone())]),
                Judgement::Refuse(_)
            ),
            "four characters do not pin a commit"
        );
        assert!(matches!(
            judge(&[read(Head::Hidden, stranger)]),
            Judgement::NeedsGrant(_)
        ));
    }

    #[test]
    fn a_merge_that_cannot_be_read_counts_as_covered() {
        let unread = |head| Judged::Unread {
            what: "the pull request".into(),
            head,
        };
        assert!(matches!(
            judge(&[unread(Head::Named("35e32938".into()))]),
            Judgement::NeedsGrant(_)
        ));
        assert!(matches!(
            judge(&[unread(Head::Unnamed)]),
            Judgement::Refuse(_)
        ));
        assert_eq!(judge(&[]), Judgement::Pass);
        assert!(
            matches!(
                judge(&[
                    read(Head::Unnamed, pull("miopea", "miopea", true)),
                    read(Head::Named("35e32938".into()), pull("x", "miopea", true)),
                ]),
                Judgement::NeedsGrant(_)
            ),
            "one covered merge in a command is enough"
        );
    }

    /// A gh that answers for the operator's account, one repository and one
    /// pull request, and records every call it gets. It colours its JSON
    /// unless colour is cleared, as gh does under `CLICOLOR_FORCE` or
    /// `GH_FORCE_TTY` and, stricter than gh 2.103.3, under `FORCE_COLOR`.
    fn stub_gh(directory: &Path, author: &str, visibility: &str) -> GhCli {
        let pull = json!({
            "author": { "login": author, "is_bot": author.starts_with("app/") },
            "headRefOid": "35e32938c0ffee",
            "number": 7,
            "url": "https://github.com/miopea/swarm-next/pull/7",
        });
        let repository = json!({ "owner": { "login": "miopea" }, "visibility": visibility });
        script_gh(
            directory,
            &format!(
                "colour() {{ if [ -n \"$NO_COLOR\" ] && [ -z \"$CLICOLOR_FORCE$GH_FORCE_TTY$FORCE_COLOR\" ]; then cat; else printf '\\033[1;37m'; cat; printf '\\033[m'; fi; }}\ncase \"$1 $2\" in\n  'api user') echo '{{\"login\":\"miopea\"}}' | colour ;;\n  'pr view') echo '{pull}' | colour ;;\n  'repo view') echo '{repository}' | colour ;;\n  *) exit 1 ;;\nesac\n"
            ),
        )
    }

    /// A gh that runs `body` after recording the call.
    fn script_gh(directory: &Path, body: &str) -> GhCli {
        use std::os::unix::fs::PermissionsExt;
        let calls = directory.join("calls");
        let script = directory.join("gh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"$PWD $*\" >> '{}'\n{body}",
                calls.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        GhCli { program: script }
    }

    fn grant_config(directory: &Path, url: &str) -> PathBuf {
        let path = directory.join("worker.json");
        std::fs::write(
            &path,
            json!({ "mcpServers": { "swarm": { "env": {
                "SWARM_MCP_URL": url,
                "SWARM_MCP_AUTHORIZATION": "Bearer worker-token"
            } } } })
            .to_string(),
        )
        .unwrap();
        path
    }

    /// Answers one grant question, and says what it was asked.
    async fn grant_answer(granted: bool) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let served = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some(split) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            let body = json!({ "granted": granted }).to_string();
            let answer = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(answer.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, served)
    }

    fn input(command: &str, cwd: &Path) -> Vec<u8> {
        json!({ "command": command, "cwd": cwd })
            .to_string()
            .into_bytes()
    }

    /// The task's own test: a stranger's pull request in the operator's
    /// public repository is refused, and merges only once the operator granted
    /// that exact command, PR and head. The operator's own pull request, and a
    /// private repository's, are not asked about at all.
    #[tokio::test]
    async fn a_strangers_pull_request_merges_only_on_the_operators_grant() {
        let directory = tempfile::tempdir().unwrap();
        let command = "gh pr merge 7 --squash --match-head-commit 35e32938c0ffee";
        let stranger = stub_gh(directory.path(), "a-stranger", "PUBLIC");
        let unanswered = grant_config(directory.path(), "http://127.0.0.1:9/mcp");

        let (url, served) = grant_answer(false).await;
        let config = grant_config(directory.path(), &url);
        let Verdict::Refuse(why) =
            gate(&config, &input(command, directory.path()), &stranger).await
        else {
            panic!("a stranger's pull request is refused before the operator grants it");
        };
        assert!(why.contains("opened by a-stranger, not by miopea"), "{why}");
        assert!(why.contains("swarm_request_decision"), "{why}");
        let asked = served.await.unwrap();
        assert!(asked.starts_with("POST /mcp/command-grant "), "{asked}");
        assert!(
            asked.contains(command),
            "the grant asked for is this exact command: {asked}"
        );
        assert!(
            asked.contains(r#""operator_only":true"#),
            "and only the operator's own grant counts, never Queen's: {asked}"
        );

        let (url, served) = grant_answer(true).await;
        let config = grant_config(directory.path(), &url);
        assert_eq!(
            gate(&config, &input(command, directory.path()), &stranger).await,
            Verdict::Granted,
            "once the operator granted exactly this command"
        );
        served.await.unwrap();

        let calls = std::fs::read_to_string(directory.path().join("calls")).unwrap();
        assert!(
            calls.contains(&format!(
                "{} pr view 7 --json author,headRefOid,number,url",
                directory.path().canonicalize().unwrap().display()
            )),
            "gh is asked in the folder the merge runs in: {calls}"
        );

        let own = stub_gh(directory.path(), "miopea", "PUBLIC");
        assert_eq!(
            gate(
                &unanswered,
                &input("gh pr merge 7 --squash", directory.path()),
                &own
            )
            .await,
            Verdict::Pass,
            "the operator's own pull request merges as before, with no grant asked"
        );
        let private = stub_gh(directory.path(), "a-friend", "PRIVATE");
        assert_eq!(
            gate(
                &unanswered,
                &input("gh pr merge 7", directory.path()),
                &private
            )
            .await,
            Verdict::Pass,
            "a private repository is already shared with people the operator chose"
        );
        assert_eq!(
            gate(
                &unanswered,
                &input("gh pr view 7", directory.path()),
                &stranger
            )
            .await,
            Verdict::Pass
        );
    }

    /// gh colours its JSON under `CLICOLOR_FORCE`, even into a pipe, and
    /// `NO_COLOR` does not override it. A worker's session can carry it, and
    /// the gate runs gh in that session's environment, so the merging
    /// account's own pull request in a private repository was refused as one
    /// gh could not answer for.
    #[tokio::test]
    async fn gh_is_run_without_colour_whatever_the_workers_environment_says() {
        let directory = tempfile::tempdir().unwrap();
        let own = stub_gh(directory.path(), "miopea", "PRIVATE");
        // Set or not in this test's own environment, gh never sees them.
        let command = own.command(directory.path(), &["api", "user"]);
        let set: Vec<(String, Option<String>)> = command
            .as_std()
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        for (name, value) in [
            ("CLICOLOR_FORCE", None),
            ("GH_FORCE_TTY", None),
            ("FORCE_COLOR", None),
            ("NO_COLOR", Some("1")),
        ] {
            assert!(
                set.contains(&(name.to_owned(), value.map(str::to_owned))),
                "{name} in {set:?}"
            );
        }

        let merge = only_merge("gh pr merge 7", directory.path().to_str().unwrap());
        let read = facts(&own, &merge)
            .await
            .expect("gh's answer is JSON once colour is off");
        assert_eq!((read.author.as_str(), read.public), ("miopea", false));

        let unanswered = grant_config(directory.path(), "http://127.0.0.1:9/mcp");
        assert_eq!(
            gate(
                &unanswered,
                &input("gh pr merge 7 --squash", directory.path()),
                &own
            )
            .await,
            Verdict::Pass,
            "the merging account's own pull request in a private repository"
        );
    }

    /// The worker's three commands open a pull request without GitHub being
    /// asked anything, while a merge hidden in text that cannot be read is
    /// still refused.
    #[tokio::test]
    async fn a_pull_request_whose_text_says_merge_opens_and_a_hidden_merge_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let unanswered = grant_config(directory.path(), "http://127.0.0.1:9/mcp");
        let missing = GhCli {
            program: directory.path().join("no-such-gh"),
        };
        for command in OPENS_A_PULL_REQUEST {
            assert_eq!(
                gate(&unanswered, &input(command, directory.path()), &missing).await,
                Verdict::Pass,
                "{command}"
            );
        }
        for (hidden, why_refused) in [
            ("gh pr merge 5 --subject \"it's", "cannot be read"),
            (
                "bash <<'EOF'\ngh pr merge 5 --subject \"it's\nEOF",
                "cannot be read",
            ),
            (
                "cat > m.sh <<'EOF'\ngh pr merge 5\nEOF\n./m.sh",
                "gh could not be run",
            ),
        ] {
            let Verdict::Refuse(why) =
                gate(&unanswered, &input(hidden, directory.path()), &missing).await
            else {
                panic!("a merge hidden in a heredoc is refused: {hidden}");
            };
            assert!(why.contains(why_refused), "{why}");
        }
    }

    #[tokio::test]
    async fn anything_that_cannot_be_checked_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let unanswered = grant_config(directory.path(), "http://127.0.0.1:9/mcp");
        let missing = GhCli {
            program: directory.path().join("no-such-gh"),
        };
        let command = "gh pr merge 7 --match-head-commit 35e32938c0ffee";

        let Verdict::Refuse(why) =
            gate(&unanswered, &input(command, directory.path()), &missing).await
        else {
            panic!("a pull request gh cannot read is refused");
        };
        assert!(
            why.contains("could not read who opened it (gh could not be run"),
            "{why}"
        );
        assert!(
            !why.contains("counts it as someone else's"),
            "an answer it could not read is not dressed as a stranger's pull request: {why}"
        );
        assert!(why.contains("Swarm could not be asked"), "{why}");

        let garbled = script_gh(directory.path(), "echo 'not json'\n");
        let Verdict::Refuse(why) =
            gate(&unanswered, &input(command, directory.path()), &garbled).await
        else {
            panic!("an answer gh gives that is not JSON is refused");
        };
        assert!(
            why.contains("could not read who opened it (gh's answer was not JSON)"),
            "{why}"
        );

        assert!(matches!(
            gate(&unanswered, b"not json", &missing).await,
            Verdict::Refuse(_)
        ));
        let stranger = stub_gh(directory.path(), "a-stranger", "PUBLIC");
        assert!(matches!(
            gate(
                &unanswered,
                &input("gh pr merge 'open", directory.path()),
                &stranger
            )
            .await,
            Verdict::Refuse(_)
        ));
        assert_eq!(Verdict::Refuse("why".into()).printed(), "refuse\nwhy");
        assert_eq!(Verdict::Granted.printed(), "granted");
        assert_eq!(Verdict::Pass.printed(), "pass");
    }
}
