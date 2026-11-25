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
    /// such part is the value of a text flag (`holds_text`).
    Printed {
        word: String,
        splits: bool,
        as_text: bool,
    },
    /// A process substitution: commands that read or write through a file
    /// the gate does not follow.
    Opaque,
    /// Anything that ends one pipeline: `;`, `&`, `||`, a newline, a
    /// parenthesis, or the edge of a command substitution.
    Break,
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
    let mut lexer = Lexer {
        in_substitution: substitution,
        ..Lexer::default()
    };
    let mut depth = 0_usize;
    while let Some(character) = chars.next() {
        match character {
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
                lexer.boundary();
                lexer.read_heredocs(chars)?;
            }
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                lexer.boundary();
            }
            '|' => {
                // `|&` pipes standard error as well.
                if chars.peek() == Some(&'&') {
                    chars.next();
                }
                lexer.pipe();
            }
            ';' | '&' => lexer.boundary(),
            // With extglob on, `!(`, `@(`, `+(`, `*(` and `?(` start a
            // pattern, which this does not read.
            '(' if lexer.word.ends_with(['!', '@', '+', '*', '?']) => return None,
            // `(( ... ))` is arithmetic, where `<<` is a shift. When bash
            // reads it otherwise, it is two subshells.
            '(' if chars.peek() == Some(&'(') => {
                lexer.boundary();
                match arithmetic(chars)? {
                    Parenthesis::Arithmetic { length, names } => {
                        chars.nth(length - 1);
                        lexer.evaluates(names);
                    }
                    Parenthesis::Commands => depth += 1,
                }
            }
            '(' => {
                depth += 1;
                lexer.boundary();
            }
            ')' if substitution && depth == 0 => {
                lexer.end();
                return lexer.heredocs.is_empty().then_some(lexer.tokens);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                lexer.boundary();
            }
            '`' => lexer.substitution(tokens(&take_backquoted(chars)?)?, false)?,
            '$' if chars.next_if_eq(&'(').is_some() => {
                lexer.started = true;
                match dollar_parenthesis(chars)? {
                    Dollar::Substitution(inner) => lexer.substitution(inner, false)?,
                    Dollar::Arithmetic { names } => lexer.evaluates(names),
                }
            }
            '$' if evaluates_a_value(chars) => {
                lexer.evaluates(true);
                lexer.started = true;
                lexer.word.push('$');
            }
            '$' if chars.next_if_eq(&'\'').is_some() => lexer.ansi_c_quoted(chars)?,
            // `$"..."` is the quoted text as translated.
            '$' if chars.peek() == Some(&'"') => {
                lexer.computed(true);
                lexer.word.push('$');
            }
            '$' if chars.peek().copied().is_some_and(parameter) => {
                lexer.computed(false);
                lexer.word.push('$');
            }
            '<' | '>' => lexer.redirection(character, chars)?,
            '#' if !lexer.started => {
                while chars.peek().is_some_and(|next| *next != '\n') {
                    chars.next();
                }
            }
            other => lexer.unquoted(other, chars.peek().copied()),
        }
    }
    // A substitution the text never closes, or a heredoc whose body never
    // came: what the shell would run cannot be told.
    lexer.end();
    (!substitution && lexer.heredocs.is_empty()).then_some(lexer.tokens)
}

/// What bash computes in a word.
#[derive(Clone, Copy)]
struct Printing {
    /// A part of it may be any number of words, so the word splits.
    splits: bool,
    /// Every part of it is the value of a text flag.
    as_text: bool,
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
    /// The next word is a redirection's file, not part of the command.
    redirect_target: bool,
    /// Heredocs on the current line, in order: their bodies start after the
    /// next newline.
    heredocs: Vec<Heredoc>,
    /// The commands of this command's substitutions, which follow it once
    /// it ends, so that its own words stay together.
    substituted: Vec<Token>,
    /// Reading the inside of a `$(`, where bash ends a heredoc early.
    in_substitution: bool,
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
                '$' if evaluates_a_value(chars) => {
                    self.evaluates(true);
                    self.word.push('$');
                }
                '$' if chars.peek().copied().is_some_and(parameter) => {
                    self.computed(true);
                    self.word.push('$');
                }
                other => self.word.push(other),
            }
        }
    }

    /// The rest of a `$'...'` string, whose opening has been read. Bash
    /// reads its escapes, so a backslash keeps a quote from ending it.
    fn ansi_c_quoted(&mut self, chars: &mut Chars<'_>) -> Option<()> {
        self.computed(true);
        self.word.push('$');
        loop {
            match chars.next()? {
                '\'' => return Some(()),
                '\\' => {
                    self.word.push('\\');
                    self.word.push(chars.next()?);
                }
                other => self.word.push(other),
            }
        }
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
        if first == '<' && chars.peek() == Some(&'<') {
            chars.next();
            if chars.peek() != Some(&'<') {
                let strip_tabs = chars.next_if_eq(&'-').is_some();
                let (delimiter, quoted) = heredoc_delimiter(chars)?;
                self.heredoc(delimiter, strip_tabs, quoted, standard);
                return Some(());
            }
            // A here-string's word is passed over, as it always was; ADR
            // 0188 names that limit.
        }
        while matches!(chars.peek(), Some('<' | '>')) {
            chars.next();
        }
        let substitution = chars.peek() == Some(&'(');
        if substitution {
            self.tokens.push(Token::Opaque);
        }
        if (first == '<' && !substitution) || onto_input {
            self.tokens.push(Token::Input);
        }
        if chars.next_if_eq(&'&').is_some() {
            while chars
                .peek()
                .is_some_and(|next| next.is_ascii_digit() || *next == '-')
            {
                chars.next();
            }
        } else {
            self.redirect_target = true;
        }
        Some(())
    }

    fn pipe(&mut self) {
        self.finish();
        self.tokens.push(Token::Pipe);
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
    fn read_heredocs(&mut self, chars: &mut Chars<'_>) -> Option<()> {
        for heredoc in std::mem::take(&mut self.heredocs) {
            self.tokens[heredoc.at] = Token::Stdin {
                body: heredoc_body(chars, &heredoc, self.in_substitution)?,
                expands: !heredoc.quoted,
                standard: heredoc.standard,
            };
        }
        Some(())
    }

    fn finish(&mut self) {
        self.braces = 0;
        self.brackets = 0;
        self.quotes = 0;
        if self.started {
            let word = std::mem::take(&mut self.word);
            let printed = self.printed.take();
            if self.redirect_target {
                self.redirect_target = false;
            } else {
                self.tokens.push(match printed {
                    Some(Printing { splits, as_text }) => Token::Printed {
                        word,
                        splits,
                        as_text,
                    },
                    None => Token::Word(word),
                });
            }
            self.started = false;
        }
    }

    /// The end of a pipeline, followed by the commands of its substitutions.
    fn boundary(&mut self) {
        self.finish();
        self.tokens.push(Token::Break);
        self.tokens.append(&mut self.substituted);
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
        for token in inner {
            match token {
                Token::Stdin { body, .. } if !text => {
                    self.substituted.push(Token::Break);
                    self.substituted.extend(tokens(&body)?);
                    self.substituted.push(Token::Break);
                }
                other => self.substituted.push(other),
            }
        }
        self.substituted.push(Token::Break);
        Some(())
    }

    /// Bash computes the part of the word now read as it runs the command:
    /// what a substitution prints, a variable, `~`, `$'...'` or `$"..."`, a
    /// brace list or a glob. It may be any word, so for gh and git it counts
    /// only as a text flag's value (`holds_text`), and only when it stays
    /// `whole`, one word. Returns whether it is that value (ADR 0188).
    fn computed(&mut self, whole: bool) -> bool {
        let slot = self.in_text_flag();
        let before = self.printed.unwrap_or(Printing {
            splits: false,
            as_text: true,
        });
        self.started = true;
        self.printed = Some(Printing {
            splits: before.splits || !whole,
            as_text: before.as_text && slot,
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

/// After `$`: `${...}` or `$[...]`. Either can read a value as a name or as
/// arithmetic (`${!x}`, `${x@P}`, `${a[i]}`, `${x:i}`, `$[x]`), where a
/// subscript runs commands, so neither is read.
fn evaluates_a_value(chars: &Chars<'_>) -> bool {
    matches!(chars.clone().next(), Some('{' | '['))
}

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
    merges_within(command, cwd, depth, true)
}

/// The merges in text a command runs or reads. `plain` says that nothing
/// around the text can run what a heredoc in it gives a command.
fn merges_within(text: &str, cwd: Option<&Path>, depth: usize, plain: bool) -> Parsed {
    let mentions_merge = text.to_ascii_lowercase().contains("merge");
    match tokens(text).filter(|_| depth <= MAX_NESTING) {
        Some(tokens) => merges_among(&tokens, cwd, depth, plain),
        None if mentions_merge => Parsed::Unclear,
        None => Parsed::Merges(Vec::new()),
    }
}

/// The merges among one command's tokens.
///
/// A heredoc's body is read as commands, as a file written from it may be
/// run later. A body that cannot be read as commands is left as data only
/// when no command here, nor any around it, can run what it is given, with
/// what an unquoted one expands still read (ADR 0188).
fn merges_among(tokens: &[Token], cwd: Option<&Path>, depth: usize, plain: bool) -> Parsed {
    let data = plain && runs_nothing_it_is_given(tokens);
    let mut folder = cwd.map(Path::to_path_buf);
    let mut merges = Vec::new();
    for pipeline in tokens.split(|token| *token == Token::Break) {
        let commands: Vec<Simple<'_>> = pipeline
            .split(|token| *token == Token::Pipe)
            .map(Simple::from)
            .collect();
        for (place, command) in commands.iter().enumerate() {
            let input = seen_input(&commands, place);
            let mut read = Vec::new();
            match step(&command.words, input.as_deref()) {
                Step::ChangeFolder(target) => folder = moved(folder, target.as_deref()),
                Step::Merge(mut merge) => {
                    merge.folder.clone_from(&folder);
                    merges.push(merge);
                }
                Step::Nested(text) => read.push(text),
                Step::Other => {}
            }
            for &(body, expands) in &command.heredocs {
                let found = match merges_within(body, folder.as_deref(), depth + 1, data) {
                    Parsed::Unclear if data && expands => expansions(body)
                        .map_or(Parsed::Unclear, |run| {
                            merges_among(&run, folder.as_deref(), depth + 1, data)
                        }),
                    Parsed::Unclear if data => Parsed::Merges(Vec::new()),
                    found => found,
                };
                match found {
                    Parsed::Merges(inner) => merges.extend(inner),
                    Parsed::Unclear => return Parsed::Unclear,
                }
            }
            for text in read {
                match merges_within(&text, folder.as_deref(), depth + 1, data) {
                    Parsed::Merges(inner) => merges.extend(inner),
                    Parsed::Unclear => return Parsed::Unclear,
                }
            }
        }
    }
    Parsed::Merges(merges)
}

/// One simple command: its words, each heredoc it is given with whether that
/// heredoc expands, the ones it reads on standard input, and whether its
/// standard input comes from a file, a here-string or a descriptor.
struct Simple<'a> {
    words: Vec<String>,
    heredocs: Vec<(&'a str, bool)>,
    input: Vec<&'a str>,
    redirected: bool,
}

impl<'a> From<&'a [Token]> for Simple<'a> {
    fn from(tokens: &'a [Token]) -> Self {
        let mut command = Simple {
            words: Vec::new(),
            heredocs: Vec::new(),
            input: Vec::new(),
            redirected: false,
        };
        for token in tokens {
            match token {
                Token::Word(word) | Token::Printed { word, .. } => command.words.push(word.clone()),
                Token::Stdin {
                    body,
                    expands,
                    standard,
                } => {
                    command.heredocs.push((body, *expands));
                    if *standard {
                        command.input.push(body);
                    }
                }
                Token::Input => command.redirected = true,
                Token::Opaque | Token::Break | Token::Pipe => {}
            }
        }
        command
    }
}

/// What a command reads on standard input, as far as the gate can see it:
/// its own heredocs on standard input, or those the commands before it pass
/// on down the pipe as written. `None` is input it cannot see: a file, a
/// here-string or a descriptor, a command that prints anything else, or the
/// shell's own. A heredoc on another descriptor is none of these.
fn seen_input(commands: &[Simple<'_>], place: usize) -> Option<Vec<String>> {
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
        if !passes_on(&commands[at].words) {
            return None;
        }
    }
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

/// Whether no command here can run what it is given or a file it is named:
/// no process substitution or value read as a name, and every command one
/// of a few that print, store or send what they read. That includes the
/// commands an unquoted heredoc runs as it is read (ADR 0188).
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
    "cat", "tee", "echo", "printf", "cd", "pwd", "true", "false", ":", "mkdir", "ls", "rm",
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
    Merge(Merge),
    /// Text a shell runs as commands: `bash -c '...'`, `eval ...`.
    Nested(String),
    Other,
}

/// What one simple command does, from its words and what it reads on
/// standard input; `None` is input the gate cannot see.
fn step(words: &[String], input: Option<&[String]>) -> Step {
    let words = program(words);
    let Some(name) = words
        .first()
        .map(|first| first.rsplit('/').next().unwrap_or(first))
    else {
        return Step::Other;
    };
    let arguments = &words[1..];
    match name {
        "cd" => Step::ChangeFolder(
            arguments
                .iter()
                .find(|argument| !matches!(argument.as_str(), "-P" | "-L" | "-e" | "-@"))
                .cloned(),
        ),
        "bash" | "sh" | "zsh" | "dash" => arguments
            .iter()
            .position(|argument| {
                argument.starts_with('-') && !argument.starts_with("--") && argument.contains('c')
            })
            .and_then(|flag| arguments.get(flag + 1))
            .map_or(Step::Other, |text| Step::Nested(text.clone())),
        "eval" => Step::Nested(arguments.join(" ")),
        "xargs" => {
            let rest = arguments
                .iter()
                .position(|argument| !argument.starts_with('-'))
                .map_or(&[][..], |start| &arguments[start..]);
            // xargs reads standard input itself, for its command's words.
            match step(rest, Some(&[])) {
                Step::Merge(mut merge) => {
                    merge.unreadable = Some("its number arrives on standard input");
                    Step::Merge(merge)
                }
                _ => Step::Other,
            }
        }
        "gh" => gh(arguments, input).map_or(Step::Other, Step::Merge),
        "curl" => curl(arguments, input).map_or(Step::Other, Step::Merge),
        _ => Step::Other,
    }
}

/// `-` or `/dev/stdin`, as a file a request body is read from.
fn is_standard_input(file: &str) -> bool {
    matches!(file, "-" | "/dev/stdin")
}

/// The words from the program on: assignments and wrappers that run the rest
/// as a command are passed over.
fn program(words: &[String]) -> &[String] {
    let mut rest = words;
    loop {
        match rest.first().map(String::as_str) {
            Some(word) if is_assignment(word) => rest = &rest[1..],
            Some("env" | "command" | "builtin" | "exec" | "nohup" | "time" | "sudo" | "nice") => {
                rest = &rest[1..];
                while rest.first().is_some_and(|word| word.starts_with('-')) {
                    rest = &rest[1..];
                }
            }
            _ => return rest,
        }
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
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
/// input the gate cannot see.
fn gh(arguments: &[String], input: Option<&[String]>) -> Option<Merge> {
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
        (Some("api"), Some(endpoint)) if graphql_endpoint(endpoint) => {
            let request = || line.fields.iter().chain(body.unwrap_or_default());
            if request().any(|part| names_a_merge_mutation(part)) {
                Some(graphql_merge(
                    request().any(|part| part.contains("expectedHeadOid")),
                ))
            } else if body.is_none() || !line.files.is_empty() {
                Some(unseen_request())
            } else {
                None
            }
        }
        (Some("api"), Some(endpoint)) => {
            let (repo, number) = merge_endpoint(endpoint)?;
            line.method
                .as_deref()
                .is_some_and(|method| method.eq_ignore_ascii_case("PUT"))
                .then(|| Merge {
                    folder: None,
                    repo,
                    pull_request: Some(number),
                    head: match line
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
                    },
                    unreadable: None,
                })
        }
        _ => None,
    }
}

/// `input` is what the command reads on standard input, which curl sends as
/// data given as `@-`. `None` is input the gate cannot see.
fn curl(arguments: &[String], input: Option<&[String]>) -> Option<Merge> {
    let mut method = None;
    let mut data = Vec::new();
    let mut urls = Vec::new();
    // Data read from a file, or from input the gate cannot see.
    let mut unseen = false;
    // A config file, which can hold the URL and the data.
    let mut configured = false;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-X" | "--request" => method = words.next().cloned(),
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--json" => {
                if let Some(value) = words.next() {
                    // `--data-raw` sends an `@` as it is.
                    match value.strip_prefix('@').filter(|_| word != "--data-raw") {
                        Some(file) if is_standard_input(file) => match input {
                            Some(input) => data.extend_from_slice(input),
                            None => unseen = true,
                        },
                        Some(_) => unseen = true,
                        None => data.push(value.clone()),
                    }
                }
            }
            "-K" | "--config" => {
                words.next();
                configured = true;
            }
            attached if attached.starts_with("-X") && attached.len() > 2 => {
                method = Some(attached[2..].to_owned());
            }
            other if !other.starts_with('-') => urls.push(other.to_owned()),
            _ => {}
        }
    }
    if data.iter().any(|body| names_a_merge_mutation(body)) {
        return Some(graphql_merge(
            data.iter().any(|body| body.contains("expectedHeadOid")),
        ));
    }
    if configured || (unseen && urls.iter().any(|url| graphql_endpoint(url))) {
        return Some(unseen_request());
    }
    let (repo, number) = urls.iter().find_map(|url| merge_endpoint(url))?;
    method
        .as_deref()
        .is_some_and(|method| method.eq_ignore_ascii_case("PUT"))
        .then(|| Merge {
            folder: None,
            repo,
            pull_request: Some(number),
            head: if data.iter().any(|body| body.contains("\"sha\"")) {
                Head::Hidden
            } else {
                Head::Unnamed
            },
            unreadable: None,
        })
}

fn names_a_merge_mutation(text: &str) -> bool {
    [
        "mergePullRequest",
        "enablePullRequestAutoMerge",
        "enqueuePullRequest",
    ]
    .iter()
    .any(|mutation| text.contains(mutation))
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
        let path = path.trim_end_matches('/').to_ascii_lowercase();
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

/// `repos/<owner>/<name>/pulls/<number>/merge`, as a path or a URL. The
/// repository is `None` when gh fills it in from the folder (`{owner}`).
fn merge_endpoint(text: &str) -> Option<(Option<String>, String)> {
    let path = text.split(['?', '#']).next()?;
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
                "cd", "/a", "<break>", "<break>", "gh", "pr", "view", "1", "<break>", "gh", "pr",
                "merge", "2", "<pipe>", "cat"
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
                "echo", "", "", "x", "<break>", "<break>", "<opaque>", "<break>", "<break>", "z"
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
        let piped = only_merge("echo 3 | xargs gh pr merge --squash", "/srv/next");
        assert!(piped.unreadable.is_some(), "the number arrives on stdin");

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
            "echo \"$(cat <<'EOF'\ngh pr merge 5\nEOF\n)\" | bash",
            "eval \"$(cat <<'EOF'\ngh pr merge 5\nEOF\n)\"",
            "python3 - <<'EOF'\ngh pr merge 5\nEOF",
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
            "python3 - <<'EOF'\nprint('don't merge')\nEOF",
            "eval \"$(cat <<'EOF'\nit's a merge\nEOF\n)\"",
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
                "echo", "", "", "x", "<break>", "<break>", "<opaque>", "<break>", "<break>", "z"
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
    /// substitution prints may be: a variable, `~`, `$'...'` or `$"..."`, a
    /// brace list or a glob. For gh, git and printf it counts only as the
    /// quoted value of a text flag (ADR 0188).
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
                "<printed:$i>",
                "<printed:$j>",
                "<printed:k$\\'>",
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
        assert_eq!(
            words("echo $'\\'' ; x #'"),
            ["echo", "<printed:$\\'>", "<break>", "x"]
        );
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
