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
//! `bash -c`, `eval` or a substitution. A script file, an alias, or a query
//! read from a file is not seen, and neither is a session without Swarm's
//! grant mod. ADR 0179 names what that leaves uncovered.

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
    /// It mentions a merge and does not parse: quotes left open, or nesting
    /// deeper than `MAX_NESTING`.
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
    /// Anything that ends one simple command: `;`, `&`, `|`, a newline, a
    /// parenthesis, or the edge of a command substitution.
    Break,
}

/// Splits shell text into words and command boundaries, as far as deciding
/// what runs needs: quotes, escapes, comments, redirections and command
/// substitution, whose inside is read as commands of its own.
///
/// `None` when a quote or a substitution is never closed.
fn tokens(text: &str) -> Option<Vec<Token>> {
    let mut lexer = Lexer::default();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\'' => {
                lexer.started = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        other => lexer.word.push(other),
                    }
                }
            }
            '"' => {
                lexer.started = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => {
                            let escaped = chars.next()?;
                            if !matches!(escaped, '"' | '\\' | '$' | '`' | '\n') {
                                lexer.word.push('\\');
                            }
                            if escaped != '\n' {
                                lexer.word.push(escaped);
                            }
                        }
                        // The rest of the quote is still one word.
                        '`' => {
                            lexer.substitution(&take_backquoted(&mut chars)?)?;
                            lexer.started = true;
                        }
                        '$' if chars.peek() == Some(&'(') => {
                            chars.next();
                            lexer.substitution(&take_parenthesised(&mut chars)?)?;
                            lexer.started = true;
                        }
                        other => lexer.word.push(other),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') | None => {}
                Some(escaped) => {
                    lexer.started = true;
                    lexer.word.push(escaped);
                }
            },
            ' ' | '\t' => lexer.finish(),
            '\n' | ';' | '&' | '|' | '(' | ')' => lexer.boundary(),
            '`' => lexer.substitution(&take_backquoted(&mut chars)?)?,
            '$' if chars.peek() == Some(&'(') => {
                chars.next();
                lexer.boundary();
            }
            '<' | '>' => {
                // `2>` names a descriptor, not a word of the command.
                if lexer.started && lexer.word.chars().all(|digit| digit.is_ascii_digit()) {
                    lexer.word.clear();
                    lexer.started = false;
                } else {
                    lexer.finish();
                }
                while matches!(chars.peek(), Some('<' | '>')) {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    chars.next();
                    while chars
                        .peek()
                        .is_some_and(|next| next.is_ascii_digit() || *next == '-')
                    {
                        chars.next();
                    }
                } else {
                    lexer.redirect_target = true;
                }
            }
            '#' if !lexer.started => {
                while chars.peek().is_some_and(|next| *next != '\n') {
                    chars.next();
                }
            }
            other => {
                lexer.started = true;
                lexer.word.push(other);
            }
        }
    }
    lexer.finish();
    Some(lexer.tokens)
}

#[derive(Default)]
struct Lexer {
    tokens: Vec<Token>,
    word: String,
    /// A word has begun, even an empty quoted one.
    started: bool,
    /// The next word is a redirection's file, not part of the command.
    redirect_target: bool,
}

impl Lexer {
    fn finish(&mut self) {
        if self.started {
            let word = std::mem::take(&mut self.word);
            if self.redirect_target {
                self.redirect_target = false;
            } else {
                self.tokens.push(Token::Word(word));
            }
            self.started = false;
        }
    }

    fn boundary(&mut self) {
        self.finish();
        self.tokens.push(Token::Break);
    }

    /// The commands inside a substitution run too, so they are read as
    /// commands of their own, between boundaries.
    fn substitution(&mut self, inner: &str) -> Option<()> {
        self.boundary();
        self.tokens.extend(tokens(inner)?);
        self.tokens.push(Token::Break);
        Some(())
    }
}

fn take_backquoted(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut inner = String::new();
    loop {
        match chars.next()? {
            '`' => return Some(inner),
            '\\' => inner.push(chars.next()?),
            other => inner.push(other),
        }
    }
}

/// The text up to the parenthesis that closes a `$(`, which has been read.
fn take_parenthesised(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut inner = String::new();
    let mut depth = 1_usize;
    loop {
        let next = chars.next()?;
        match next {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(inner);
                }
            }
            _ => {}
        }
        inner.push(next);
    }
}

/// Every merge `command` makes, and the folder each runs in.
fn merges_in(command: &str, cwd: Option<&Path>, depth: usize) -> Parsed {
    let mentions_merge = command.to_ascii_lowercase().contains("merge");
    let Some(tokens) = tokens(command).filter(|_| depth <= MAX_NESTING) else {
        return if mentions_merge {
            Parsed::Unclear
        } else {
            Parsed::Merges(Vec::new())
        };
    };
    let mut folder = cwd.map(Path::to_path_buf);
    let mut merges = Vec::new();
    for words in tokens.split(|token| *token == Token::Break) {
        let words: Vec<String> = words
            .iter()
            .filter_map(|token| match token {
                Token::Word(word) => Some(word.clone()),
                Token::Break => None,
            })
            .collect();
        match step(&words) {
            Step::ChangeFolder(target) => folder = moved(folder, target.as_deref()),
            Step::Merge(mut merge) => {
                merge.folder.clone_from(&folder);
                merges.push(merge);
            }
            Step::Nested(text) => match merges_in(&text, folder.as_deref(), depth + 1) {
                Parsed::Merges(inner) => merges.extend(inner),
                Parsed::Unclear => return Parsed::Unclear,
            },
            Step::Other => {}
        }
    }
    Parsed::Merges(merges)
}

enum Step {
    /// `cd`, with its target; `None` is `cd` alone, which goes home.
    ChangeFolder(Option<String>),
    Merge(Merge),
    /// Text a shell runs as commands: `bash -c '...'`, `eval ...`.
    Nested(String),
    Other,
}

fn step(words: &[String]) -> Step {
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
            match step(rest) {
                Step::Merge(mut merge) => {
                    merge.unreadable = Some("its number arrives on standard input");
                    Step::Merge(merge)
                }
                _ => Step::Other,
            }
        }
        "gh" => gh(arguments).map_or(Step::Other, Step::Merge),
        "curl" => curl(arguments).map_or(Step::Other, Step::Merge),
        _ => Step::Other,
    }
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
}

fn gh_line(arguments: &[String]) -> GhLine {
    let mut line = GhLine::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let (flag, attached) = if let Some((flag, value)) = word.split_once('=')
            && flag.starts_with("--")
        {
            (flag, Some(value.to_owned()))
        } else if word.len() > 2
            && word.starts_with('-')
            && !word.starts_with("--")
            && word.is_char_boundary(2)
            && GH_VALUE_FLAGS.contains(&&word[..2])
        {
            (&word[..2], Some(word[2..].to_owned()))
        } else {
            (word.as_str(), None)
        };
        if !flag.starts_with('-') || flag == "-" {
            line.positionals.push(word.clone());
            continue;
        }
        if flag == "--disable-auto" {
            line.disables_auto_merge = true;
        }
        if !GH_VALUE_FLAGS.contains(&flag) {
            continue;
        }
        let Some(value) = attached.or_else(|| words.next().cloned()) else {
            continue;
        };
        match flag {
            "-R" | "--repo" => line.repo = Some(value),
            "-X" | "--method" => line.method = Some(value),
            "-f" | "-F" | "--field" | "--raw-field" => line.fields.push(value),
            "--match-head-commit" => line.head = Some(value),
            _ => {}
        }
    }
    line
}

fn gh(arguments: &[String]) -> Option<Merge> {
    let line = gh_line(arguments);
    let positional = |index: usize| line.positionals.get(index).map(String::as_str);
    match (positional(0), positional(1)) {
        (Some("pr"), Some("merge")) if !line.disables_auto_merge => Some(Merge {
            folder: None,
            repo: line.repo.clone(),
            pull_request: line.positionals.get(2).cloned(),
            head: line.head.clone().map_or(Head::Unnamed, Head::Named),
            unreadable: None,
        }),
        (Some("api"), Some("graphql")) => line
            .fields
            .iter()
            .any(|field| names_a_merge_mutation(field))
            .then(|| {
                graphql_merge(
                    line.fields
                        .iter()
                        .any(|field| field.contains("expectedHeadOid")),
                )
            }),
        (Some("api"), Some(endpoint)) => {
            let (repo, number) = merge_endpoint(endpoint)?;
            line.method
                .as_deref()
                .is_some_and(|method| method.eq_ignore_ascii_case("PUT"))
                .then(|| Merge {
                    folder: None,
                    repo,
                    pull_request: Some(number),
                    head: line
                        .fields
                        .iter()
                        .find_map(|field| field.strip_prefix("sha="))
                        .map_or(Head::Unnamed, |sha| Head::Named(sha.to_owned())),
                    unreadable: None,
                })
        }
        _ => None,
    }
}

fn curl(arguments: &[String]) -> Option<Merge> {
    let mut method = None;
    let mut data = Vec::new();
    let mut urls = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-X" | "--request" => method = words.next().cloned(),
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--json" => {
                data.extend(words.next().cloned());
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
                Token::Break => "<break>".to_owned(),
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
                "merge", "2", "<break>", "cat"
            ]
        );
        assert_eq!(
            words(r#"echo "x $(gh pr merge 3) y""#),
            [
                "echo", "x ", "<break>", "gh", "pr", "merge", "3", "<break>", " y"
            ]
        );
        assert_eq!(words("gh pr \\\nmerge 4"), ["gh", "pr", "merge", "4"]);
        assert_eq!(tokens("gh pr merge 'open"), None);
        assert_eq!(tokens(r#"echo "$(gh pr merge 5""#), None);
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
