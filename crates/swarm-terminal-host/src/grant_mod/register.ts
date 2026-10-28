import type { Register } from 'claude-code'

// Written by Swarm's worker engine for one worker each time it starts
// (ADR 0129). It lets the worker run a command the operator granted in Swarm,
// exactly as granted and once, in the session that asked for it.
const HOST: string = __SWARM_HOST__
const CONFIG: string = __SWARM_MCP_CONFIG__

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
}
