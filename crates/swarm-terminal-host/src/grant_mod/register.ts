import type { Register } from 'claude-code'

// Written by Swarm's worker engine for one worker each time it starts
// (ADR 0129). It lets the worker run a command the operator granted in Swarm,
// exactly as granted and once, in the session that asked for it. When auto
// mode refuses a command, it tells Swarm, which may ask the operator to grant
// it, and passes on what Swarm says to the worker (ADR 0130). And it refuses a
// merge of someone else's pull request in the operator's own public
// repository unless the operator granted that exact merge (ADR 0179).
const HOST: string = __SWARM_HOST__
const CONFIG: string = __SWARM_MCP_CONFIG__

// What Swarm said about a refused call, by that call, until its result
// reaches the worker. A note nobody collects goes with the oldest.
const MAX_NOTES = 16
const notes = new Map<string, string>()

// A command that may merge a pull request (ADR 0179). It only picks which
// commands Swarm reads closely; Swarm decides which of them merge, and this
// may name more than it needs to, never fewer. Each word is looked for as
// written and as bash may spell it, as Swarm does too (ADR 0190).
const MAY_MERGE = /merge|enqueuepullrequest/i
const RUNS_GITHUB = /\b(gh|curl)\b/
const NAMES_GITHUB = /gh|curl/
// Without a backslash and newline, a `$` before a quote, and every quote and
// backslash.
const withoutQuotes = (text: string): string =>
  text
    .replace(/\\\n/g, '')
    .replace(/\$(?=['"])/g, '')
    .replace(/['"\\]/g, '')
// Without what bash expands in a word, which may be empty: `${...}`, a `$(...)`
// with no parenthesis inside, a backquoted command, `$` and a name, and `$`
// and a digit or one of bash's own parameters.
const EXPANSION = /\$\{[^}]*\}|\$\([^()]*\)|`[^`]*`|\$[A-Za-z_][A-Za-z0-9_]*|\$[0-9@*#?$!-]/g
const withoutExpansions = (text: string): string => text.replace(EXPANSION, '')
// An escape that names a letter by its number, read as that letter: `\xHH`,
// `\NNN` in octal, which keeps its low byte as bash and printf do, `\uHHHH`
// and `\UHHHHHHHH`. One that makes no letter is kept as written.
const NUMBERED = String.raw`x([0-9a-fA-F]{1,2})|u([0-9a-fA-F]{1,4})|U([0-9a-fA-F]{1,8})|([0-7]{1,3})`
const numbered = (escape: string, x?: string, u?: string, wide?: string, octal?: string): string => {
  const code = octal === undefined ? parseInt(x ?? u ?? wide ?? '', 16) : parseInt(octal, 8) & 0xff
  return code <= 0x10ffff ? String.fromCodePoint(code) : escape
}
// The one-letter escapes bash's `$'...'`, printf and JSON read.
const LETTERS: Record<string, string> = {
  a: '\x07',
  b: '\b',
  e: '\x1b',
  E: '\x1b',
  f: '\f',
  n: '\n',
  r: '\r',
  t: '\t',
  v: '\v',
}
// Every escape read as its letter wherever it stands, as printf and JSON read
// them.
const ESCAPE = new RegExp(String.raw`\\(?:${NUMBERED}|([abeEfnrtv]))`, 'g')
const unescaped = (text: string): string =>
  text.replace(
    ESCAPE,
    (escape: string, x?: string, u?: string, wide?: string, octal?: string, letter?: string) =>
      letter === undefined ? numbered(escape, x, u, wide, octal) : (LETTERS[letter] ?? escape),
  )
// Each escape that names a letter by its number read as that letter,
// wherever it stands, and each other escape kept, so a backslash and a
// letter is that letter once its backslash is taken out, as bash reads it
// outside `$'...'`. Bash makes a letter only from a letter, a backslash and
// a letter, or such a number in `$'...'`, so this finds each merge it
// spells, wherever a `$'...'` string begins.
const NUMBERED_ESCAPE = new RegExp(String.raw`\\(?:${NUMBERED})`, 'g')
const byNumber = (text: string): string => text.replace(NUMBERED_ESCAPE, numbered)
// With each run of `%HH` escapes read as the text its bytes make, as a server
// may read an address's path once bash has taken its quotes out, so a quote
// or a backslash inside one, as in `%6''d`, splits nothing. A run that makes
// no UTF-8 is kept as written.
const PERCENT = /(?:%[0-9a-fA-F]{2})+/g
const percentDecoded = (text: string): string =>
  text.replace(PERCENT, (run: string) => {
    try {
      return decodeURIComponent(run)
    } catch {
      return run
    }
  })
const mayMerge = (command: unknown): command is string => {
  if (typeof command !== 'string') return false
  const bare = withoutQuotes(command)
  const read = byNumber(command)
  const spellings = [
    command,
    bare,
    percentDecoded(bare),
    percentDecoded(withoutQuotes(read)),
    withoutQuotes(withoutExpansions(read)),
    withoutQuotes(withoutExpansions(unescaped(command))),
  ]
  // A `$'...'` string may make the tab or newline before gh or curl that
  // bash splits a word at, so in a command with one, either counts anywhere.
  const github = command.includes("$'") ? NAMES_GITHUB : RUNS_GITHUB
  return spellings.some(text => MAY_MERGE.test(text)) && spellings.some(text => github.test(text))
}
const MERGE_UNCHECKED =
  'Swarm could not check this merge, so it is refused (ADR 0179). In the operator\'s own ' +
  'public repositories a merge of a pull request the account making it did not open needs ' +
  'the operator\'s grant for that exact command. Try again; if it is still refused, ask the ' +
  'operator for this exact command with swarm_request_decision. Do not merge it any other way.'

export const register: Register = on => {
  on('tool.check', { tool: 'Bash' }, async ($, e, next) => {
    const verdict = await next(e)
    // Only a call still waiting on the mode's decider can be granted. An
    // allow or a deny already stands, and an ask from a rule, a hook, a
    // setting or another plugin is someone's own choice to be asked.
    const waiting =
      verdict.decision === 'ask' &&
      !verdict.rule &&
      !verdict.hook &&
      !verdict.setting &&
      !verdict.plugin
    const command = (e.input as { command?: unknown }).command
    if (typeof command !== 'string') return verdict
    // Claude drops a leading `cd <dir> &&` when the shell is already in
    // <dir>, so Swarm is told where the shell is to recognise the granted text.
    const cwd = await $.session.cwd().catch(() => undefined)
    // A merge of someone else's pull request in the operator's own public
    // repository runs only on the operator's grant for it, whatever allowed
    // it or would have: a rule, or auto mode taking Queen's word (ADR 0179).
    // A deny stands, and a person asked decides for themselves.
    if ((waiting || verdict.decision === 'allow') && mayMerge(command)) {
      const gate = await $.process.run([HOST, 'provider-merge', CONFIG], {
        stdin: JSON.stringify({ command, cwd }),
        timeoutMs: 30_000,
      })
      const [word = '', ...why] = gate.exitCode === 0 ? gate.stdout.split('\n') : []
      if (word.trim() === 'granted') {
        return {
          decision: 'allow',
          reason: 'The operator granted this exact merge in Swarm.',
        }
      }
      if (word.trim() !== 'pass') {
        return { decision: 'deny', reason: why.join('\n').trim() || MERGE_UNCHECKED }
      }
    }
    if (!waiting) return verdict
    const asked = await $.process.run([HOST, 'provider-grant', CONFIG], {
      stdin: JSON.stringify({ command, cwd }),
      timeoutMs: 10_000,
    })
    return asked.exitCode === 0 && asked.stdout.trim() === 'granted'
      ? { decision: 'allow', reason: 'The operator granted this exact command in Swarm.' }
      : verdict
  }).catch(($, e, next) =>
    // A grant that fails leaves the worker's own checks to decide. A merge
    // check that fails refuses: its doubt is never a pass.
    mayMerge((e.input as { command?: unknown }).command)
      ? { decision: 'deny', reason: MERGE_UNCHECKED }
      : next(e),
  )

  // Auto mode refused a command: the exact text, as tool.check saw it.
  on('classic.PermissionDenied', async ($, e, next) => {
    const result = await next(e)
    const command = (e.tool_input as { command?: unknown } | null)?.command
    if (e.tool_name !== 'Bash' || e.permission_mode !== 'auto' || typeof command !== 'string') {
      return result
    }
    const cwd = await $.session.cwd().catch(() => e.cwd)
    const told = await $.process.run([HOST, 'provider-refused', CONFIG], {
      stdin: JSON.stringify({ command, cwd, reason: e.reason }),
      timeoutMs: 10_000,
    })
    const note = told.exitCode === 0 ? told.stdout.trim() : ''
    if (note) {
      notes.set(e.tool_use_id, note)
      for (const id of notes.keys()) {
        if (notes.size <= MAX_NOTES) break
        notes.delete(id)
      }
    }
    return result
  }).catch(($, e, next) => next(e))

  // The refusal the worker reads carries Swarm's words after Claude's own.
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const out = await next(e)
    const note = e.tool_use_id === undefined ? undefined : notes.get(e.tool_use_id)
    if (e.tool_use_id === undefined || note === undefined) return out
    notes.delete(e.tool_use_id)
    return out.isError && typeof out.text === 'string' ? { deny: `${out.text}\n\n${note}` } : out
  }).catch(($, e, next) => next(e))
}
