import type { Register } from 'claude-code'

// Written by Swarm's worker engine for one worker each time it starts
// (ADR 0129). It lets the worker run a command the operator granted in Swarm,
// exactly as granted and once, in the session that asked for it. When auto
// mode refuses a command, it tells Swarm, which may ask the operator to grant
// it, and passes on what Swarm says to the worker (ADR 0130).
const HOST: string = __SWARM_HOST__
const CONFIG: string = __SWARM_MCP_CONFIG__

// What Swarm said about a refused call, by that call, until its result
// reaches the worker. A note nobody collects goes with the oldest.
const MAX_NOTES = 16
const notes = new Map<string, string>()

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
    if (!waiting || typeof command !== 'string') return verdict
    // Claude drops a leading `cd <dir> &&` when the shell is already in
    // <dir>, so Swarm is told where the shell is to recognise the granted text.
    const cwd = await $.session.cwd().catch(() => undefined)
    const asked = await $.process.run([HOST, 'provider-grant', CONFIG], {
      stdin: JSON.stringify({ command, cwd }),
      timeoutMs: 10_000,
    })
    return asked.exitCode === 0 && asked.stdout.trim() === 'granted'
      ? { decision: 'allow', reason: 'The operator granted this exact command in Swarm.' }
      : verdict
  }).catch(($, e, next) => next(e))

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
