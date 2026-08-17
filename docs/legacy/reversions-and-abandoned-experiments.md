# Legacy reversions and abandoned experiments

Status: **Validated against commit bodies and touched files**

Reversions are high-value evidence because they show where a plausible fix made
the operator outcome worse, where the original diagnosis was wrong, or where an
architectural boundary overruled local cleanup. They are not proof that Swarm
Next should restore either side of the old implementation.

## Explicit reverts

### Browser terminal event handlers: two rollback passes in five minutes

The February sequence `8be14499` through `abee2b36` layered drag/drop, bracketed
paste, resize signaling, Ctrl+V interception, image paste, xterm `onBinary`, and
capture-phase pre-focus handlers onto the same terminal surface. `7b4b6f6f`
removed `onBinary` and pre-focus after disconnect, scroll, and focus regressions.
Five minutes later `afecdd99` removed all terminal-handler changes from that
session, retaining only unrelated visual branding.

**What failed:** multiple browser handlers competed with xterm's own keyboard,
mouse, focus, and binary protocols. Each local fix changed ownership of another
event path, so the combined behavior was less stable than the missing feature.

**Next disposition:** **already prevented in architecture, keep watching in
rendered tests.** One terminal component owns input, paste, file attachment,
focus, resize, and mobile gesture translation. Shared component tests are not
enough; desktop and Android proof must exercise the actual composed surface.

### systemd `KillMode=mixed`: cleanup killed the product's durable core

`5b2856ca` changed the API service to `KillMode=mixed` to remove orphan child
processes that retained the listen port. `39a232ea` reverted it the same day
because systemd then killed worker PTYs, violating the sidecar architecture.
Later service work kept `KillMode=process` and fixed stale API configuration and
process cleanup without sacrificing worker survival.

**What failed:** the process tree did not reflect ownership. API children included
the state that was meant to survive an API restart, so broad process cleanup was
architecturally destructive even though it solved the port symptom.

**Next disposition:** **outcome kept through redesign.** The worker engine is an
independent service and App/API reload is a different operation from worker-engine
replacement. Release acceptance must continue proving the terminal-host PID and
provider sessions survive an App/API swap.

### Ctrl+L interception: a symptom fix hid WebSocket saturation

`db4d2135` remapped Ctrl+L in the browser because it appeared to trigger a full
terminal refresh. `de178f9c` reverted the interception after `e11d2662` fixed the
actual cause: WebSocket queue saturation during output bursts. Ctrl+L could then
flow naturally to Claude Code.

**What failed:** the observed correlation was not the cause. Capturing a provider
shortcut made the browser carry behavior the provider already owned and would
have left the saturation defect in place.

**Next disposition:** **already prevented as a design rule.** Provider-native
shortcuts pass through unless a documented, user-visible Swarm control owns the
exact chord. Redraw and queue problems are diagnosed at their transport/render
boundary rather than by intercepting the input that exposed them.

### Speculative task preparation: disable first, restore only with identity

`8b693339` introduced speculative preparation. `d44ee3e7` disabled it hours later
after pending tasks reached unrelated workers. `6b4b061a` restored a narrower
version only behind exact target identity, rate-limit awareness, operator
inactivity, and an opt-in defaulting off.

**What failed:** an optimization acted before ownership was durable. Once the
wrong worker received context, cancellation could not make that context unseen.

**Next disposition:** **deferred.** Next requires durable assignment before wake
or brief delivery. Speculation returns only if Ring 1 demonstrates material value
and cancellation, recipient, resource, and wrong-delivery proofs exist.

## Diagnostic experiments that were intentionally abandoned

### Disabling GPU renderers caused an 11 GB regression

Release `2026.8.10.5` disabled xterm's GPU renderers while investigating Edge
crashes. `796ec558` (`2026.8.10.7`) reverted that change after the DOM renderer
grew to 11–12 GB in roughly two minutes. The renderer fallback materialized a DOM
element per terminal cell while the available counters remained flat because
they measured JavaScript heap, WebSocket traffic, or canvases—not DOM memory in
the browser process.

**Lesson:** a safer-looking fallback can have a radically worse resource model.
Measure the process and memory class that owns the suspected allocation, include
a positive control proving the metric can move, and prefer restoring known prior
behavior over compounding an unconfirmed diagnosis.

**Next disposition:** **keep as acceptance evidence.** Terminal fallback paths,
scrollback bounds, and browser-process memory require their own desktop soak;
renderer heap alone cannot clear them.

### Service-worker removal exonerated the service worker but kept the safer product

`832df47c` fixed a real unbounded Cache Storage leak and measured browser-process
memory falling from 12,316 MB to 88.8 MB. Growth later returned. `be44b4d2`
replaced the service worker with a self-unregistering cache-removal kill switch to
test whether it explained the remaining browser-process growth. It did not:
memory still grew with empty storage and no registered worker. Offline caching
remained removed because the server-rendered app did not need it and its risk no
longer justified its value.

**Lesson:** a component can contain one confirmed defect without explaining the
whole incident. A diagnostic removal may become the right product simplification
even when it exonerates the component from the remaining failure.

**Next disposition:** **outcome kept.** Next's service worker is push-focused,
not an offline application cache. Any future offline behavior needs storage
bounds, upgrade/eviction proof, and browser-process soak evidence.

### Per-event title and badge writes were suspects, not findings

`1f2ea403` instrumented the last unmeasured dashboard event channel after the
classifier changed a nearly dormant state stream into a continuous one.
`cd336be1` then removed repeated `document.title` and app-badge writes as a stated
experiment because both cross the renderer/browser-process boundary. The commit
explicitly records that no trace identified either API as the SQLite allocation
source.

**Lesson:** label hypotheses as hypotheses and retain disconfirming evidence. A
continuous event path can reveal costs that never appear under a stuck or quiet
classifier, but temporal adjacency is not attribution.

**Next disposition:** **already incorporated.** Next uses one bounded invalidation
stream, quiet steady state, change-driven presentation, and separate browser/API/
terminal-host/resource diagnostics. Ring 1 still needs sustained browser-process
observation because no renderer counter can prove the outer process is bounded.

## Cross-cutting rules from the reversion history

1. **One owner per input path.** Do not layer browser, terminal emulator, provider,
   and automation handlers over the same event without an explicit precedence
   contract.
2. **Fix the owning boundary.** A keypress, status change, or restart may expose a
   failure without causing it.
3. **Worker survival outranks API cleanup.** Process managers must follow product
   ownership, not incidental parentage.
4. **Every metric needs a positive control.** A flat counter is evidence only if
   the suspected allocation would move it.
5. **Restore known behavior when an experiment accelerates harm.** A fast revert
   is a successful safety action, not a failed feature.
6. **A confirmed sub-defect is not automatically the root cause.** Keep searching
   when the measured failure survives the fix.
7. **Optimization follows durable identity.** Speculative work cannot outrun
   recipient ownership or cancellation semantics.
