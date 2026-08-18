# Developer dogfood audit

This is the working product audit for the first real developer week. It records what a developer can prove in the rendered app, what was corrected immediately, and what still needs operator judgment. A finding is not automatically a Legacy port request.

## Developer journeys exercised

| Journey | Desktop | Phone-sized | Result |
| --- | --- | --- | --- |
| Find open work, filter by source/worker/project, inspect a task | Pass | Pass | Dense desktop rows and mobile cards expose Swarm state, Jira state, source, worker, and actions. |
| Select two Inbox messages, review merged content and images | Pass | Pass with fix | Bounded sequential fetch avoids gateway failures; attachments render. Review now returns to the top of the task draft instead of preserving an unrelated Inbox scroll position. |
| Open a task and inspect full details/attachments | Pass | Pass | Double-click opens one review/edit modal. Imported images render at their natural resolution. |
| Configure a repository worker | Pass | Pass | Repository path, provider, Queen routing description, local draft, Claude draft, autostart, and guarded removal are visible. |
| Navigate a large worker roster | Pass | Pass | Active/total count and All/Awake filter keep 30+ sleeping workers manageable. |
| Quick navigation with keyboard and long result sets | Pass with fix | Pass with fix | Alt+K and scrolling work. Result rows now size to their content rather than overlapping metadata. |
| Inspect Keeper/Member Apiary state | Pass | Pass | Apiary is supervisory; work creation remains on Tasks. Member Hives can see public Apiary structure without routine terminal noise. |
| Resize and use a terminal | Pass with live fix | Pass with live fix | Terminal owns the remaining viewport. A large canonical scrollback now shows a bounded layout-recovery state during a viewport change, then repaints at the full width without a refresh. Touch direction and Jump to latest remain real-device proofs. |
| Recover Queen automation submission | Test pass; live release pending | Same | Submission now waits for stable output, retries Enter, and proves the marker left the input instead of trusting a host acknowledgement. |
| Open an existing Queen decision from Settings | Pass | Pass | Settings recognizes the durable decision, removes the retry action, and routes to the one calm queue without starting another Queen review. |
| Find Legacy data for migration | Pass | Pass | Primary path discovers the normal local Legacy database read-only. File selection remains an advanced fallback. |
| Understand runtime updates | Pass in current UI | Pass | Worker engine and App/API are separate cards with versions, risk, and action state. Re-prove progress across a real update after release. |
| Find one worker or command in a 31-worker Hive | Pass | Pass | The Awake filter keeps the rail calm; quick navigation filters sleeping workers by name and scrolls inside its bounded dialog. Alt+K opens it from ordinary desktop controls. |

## Fixes made during this pass

1. **Queen automation submission is evidence-based.** Every output advance resets stability; Swarm retries a separated Enter and accepts only a later resting prompt after the exact marker.
2. **Legacy migration begins from the installed Hive.** The app checks the normal local Legacy database and previews it read-only. Users no longer need to locate or export a package first.
3. **Migration layering is clean.** The Legacy SQLite adapter belongs to persistence; the API and command-line exporter share it without the API depending on the CLI.
4. **Merged email review starts where the user needs to act.** The task draft is scrolled into view and focused when review opens.
5. **Mobile quick-navigation rows no longer overlap.** Grid rows size to content.
6. **Mobile terminal metadata is reduced.** The internal session identifier is hidden at phone sizes so the status and controls do not collide.
7. **Legacy history no longer buries migration work.** Closed Legacy tasks, malformed records, and records staying in Legacy are hidden from the actionable preview by default, each with its own counted disclosure control.
8. **A normal provider runtime is no longer a critical alert.** Loaded Claude and Codex process trees use the same 2/4 GiB pressure bands as automatic worker admission instead of the Rust service's smaller 256/512 MiB thresholds.
9. **Runtime identities stay useful without consuming the card.** App/API and worker-engine surfaces share one version presenter that keeps the release and short revision while dropping development timestamps and process suffixes.
10. **Large Crew settings are searchable.** A 30+ worker roster can be filtered by worker, repository, provider, or Queen-routing description. Reordering is deliberately paused while results are filtered so hidden positions cannot move unexpectedly.
11. **A pending Queen decision is not mistaken for a failed review.** Settings now presents the durable decision count and opens **Needs you** instead of offering to rerun Queen and creating a review loop.
12. **Release packaging refuses a stale browser build.** The guarded packaging path now fails when web source is newer than `web/dist`, preventing an updated API from silently shipping an older UI.
13. **Terminal resize recovery is visible instead of looking broken.** Large canonical scrollback can take several seconds to parse and refit after a phone-to-desktop transition. Swarm now keeps the terminal surface visible, marks it busy, and shows **Adjusting terminal layout…** until the full-width repaint completes.
14. **Mobile Settings deep links no longer expose clipped content above the section rail.** The sticky section control now sits flush with the Settings scroll viewport.
15. **Repository paths remain readable while editing a worker.** Long paths wrap within the mobile worker card instead of disappearing beyond its edge.
16. **Worker-engine compatibility is not described as version equality.** Runtime now says that an unchanged worker engine is compatible with the current App/API release, while continuing to show each installed revision independently.

## Live deployed proof — 2026-08-18

- Desktop and 412 × 915 phone layouts were exercised against `swarm2.bfgsolutions.net` on App/API `0.1.0-5a8a14b7d9ca`.
- The normal Legacy database was found read-only, previewed, and cancelled. Release `0.1.0-ca4d9ff2ff0e` showed 16 actionable tasks initially, with 33 malformed records, 62 records staying in Legacy, and 1,708 closed tasks behind separate disclosures. No import was committed.
- Runtime diagnostics were rechecked on `0.1.0-7d9538977d9c`: one loaded provider tree at 474.6 MiB remained **Normal**, machine memory remained separately visible, and the terminal-host service stayed at 43.9 MiB.
- Release `0.1.0-90c3ee6a20cc` kept the existing terminal-host process and Queen session attached, rendered concise App/API and worker-engine identities, and filtered the 31-worker Crew roster at 412 × 915 without horizontal overflow or hidden reorder actions.
- The same release restored the authenticated workspace in a fresh browser tab without another login, preserved the Tasks workspace across a full reload, and reattached the loaded Queen terminal. Installed-PWA process closure remains a separate real-device proof.
- Since-midnight service evidence contained zero warning-level API or terminal-host entries; both user services were active and `/health` reported the deployed App/API and worker-engine build identities.
- A merged Outlook task opened directly into its single review/edit dialog. Both images appeared, both attachment records were present, and guarded removal stayed visible without changing the task.
- The phone-sized task board filtered cleanly across Jira, email, and Swarm-created sources, showed a useful zero-result state, and kept the 17-message Inbox chooser bounded and readable.
- A long Queen terminal moved into scrollback with Shift+PageUp, exposed **Jump to latest**, and returned to the live prompt when selected.
- Quick navigation opened from its toolbar action and Alt+K, filtered a 31-worker roster down to the requested Codex workers, and kept the result list scrollable at phone size.
- **Generate with Claude** showed bounded progress, completed in roughly 30 seconds, and returned an editable routing draft with a clear save requirement. The draft was cancelled so the live worker profile was not changed.
- Repeated Tasks → Workers route changes painted the mobile terminal immediately during the re-proof. One earlier delayed compositor frame remains intermittent rather than a reproducible application failure; it is not being masked with an arbitrary delay.
- Release `0.1.0-e1ca5f59155d` displayed **Queen needs you** and one **Review decision** action in Settings, with no **Retry Queen review** action. Selecting it opened the existing approval in **Needs you** without creating or resolving any work.
- The first `8a2ba9e3` package exposed a stale-browser-build hazard during live proof: the API revision changed while the installed browser bundle did not. Packaging now rejects that condition, and the uniquely versioned `e1ca5f59` package proved the corrected browser and API together while preserving the terminal-host process.
- Release `0.1.0-aec0ba3fb9e9` preserved the terminal-host process and Queen session. A 412 × 915 → 1440 × 1000 transition exposed **Adjusting terminal layout…** while the large Queen scrollback was rebuilt, then restored the complete terminal at the new width without a refresh.
- The same release rechecked the task board, Jira image detail, Inbox chooser, mobile worker switcher, Keeper rollup, Settings deep link, and runtime identities at both phone and desktop widths without mutating tasks, workers, decisions, or external systems.

## Verified existing capabilities

- Sleeping workers are unloaded, remain in the roster, and preserve provider conversations.
- The worker editor supports Claude and Codex defaults for the next wake without rewriting history.
- Repository descriptions can be drafted locally or generated by one bounded, tool-free Claude turn; the generated text is editable and is not used by Queen until saved.
- Worker removal is explicit, preserves repository files/history, and refuses running workers or workers with open assignments.
- Jira and email tasks share the same task board and filtering model; source-specific import is only an entry path.
- Apiary pages show organization, public work rollups, claims, and delegation; private worker/repository/session data remains owned by each Hive.
- Task removal is available for local work with stronger guards for Jira-backed work.
- Desktop task details, Jira links, image attachments, and email source threads are visible without opening Jira or Outlook.
- The full Rust workspace is green: 455 unit/integration tests plus all crate documentation tests passed on the release checkout.
- Rust formatting and workspace-wide Clippy checks pass with warnings denied; the five browser-process dogfood harness tests also pass.

## Release re-proofs required

- Submit one automatic Queen review and prove it starts without a stranded pasted token or manual Enter.
- Scroll a long terminal on Android and a Windows touch screen in both directions, then use **Jump to latest**.
- Resize the worker rail repeatedly and rotate/change viewport on a real touch device; the browser-sized transition is now proven with explicit recovery and correct final geometry.
- Run a worker-engine update and verify immediate progress, worker-engine build identity, completion, and accurate timeout recovery.
- Run an App/API development reload with a changed and unchanged checkout; only the changed checkout should offer an action.
- Generate a worker routing description with Claude and verify the progress state resolves to an editable success or a specific recoverable error.
- Close and reopen both desktop and Android PWAs; authentication and the current workspace should survive.

## Catalogued refinements after dogfood blockers

- Split the main browser bundle by workspace after the functional milestone; the current production build is healthy but its shared entry chunk is about 522 kB before gzip.
- Give compute load a user-facing interpretation only after observing real Hive baselines. Do not invent a warning threshold from one four-CPU machine.
- Keep desktop and Android PWA persistence on the real-device acceptance list; a responsive browser viewport cannot prove installed-app storage behavior.
- Instrument route-to-first-paint timing before attempting another redraw workaround. During automated phone-sized proof, the accessibility tree changed immediately while captured pixels sometimes retained the previous workspace for roughly one to two seconds. The final surface was correct, but this matches the operator's intermittent stale-paint report and should be measured rather than hidden behind an arbitrary delay.

## Morning decisions

These are product choices rather than obvious repairs:

1. **Model routing defaults.** Decide whether Queen may recommend a cheaper/faster model only, or may also apply an in-provider `/model` switch while the worker is resting. Provider handover remains documentation-only.
2. **Automatic description spend.** Decide whether new workers should automatically receive the bounded Claude routing draft or keep the current explicit button and cost disclosure.
3. **Migration completion signal in Legacy.** Confirm the exact non-destructive marker Legacy should record after Next accepts a task so both apps show one authority during cutover.
4. **Night Watch deployment rules.** Approve the first exact durable deployment scopes Queen may use; Queen should delegate execution to Scout/repository workers rather than becoming a coding worker.
5. **Apiary activity density.** Choose the default Keeper/Member time window for completed-work rollups once more than one real Hive is connected.
6. **Queen approval granularity.** The current Queen can correctly group eight related Ready records into one durable decision, but that produces one large approval with coupled action buttons. The next contract should create task- or worker-scoped decision records, with an optional summary above them, instead of asking the UI to parse a model-written blob into actions.

## Not reproduced or intentionally deferred

- Cursor menus were rechecked at the actual pointer location; no offset failure reproduced in the current rendered build.
- Cross-provider live handover is intentionally deferred. Claude and Codex remain first-class; OpenCode and Gemini stay future providers until their terminal experiences meet the same bar.
- Legacy drones are not a direct port. Deterministic coordination handles no-judgment work without an LLM call; Queen handles judgment and Scout handles cross-repository execution.
