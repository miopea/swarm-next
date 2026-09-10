# Native clarification acceptance — September 10, 2026

This is evidence for ADR 0094 / the UI-1 clarification slice, not completion of
the daily-driver program. No release or BFG Admin communication occurred.

## Isolated environment

- Candidate: committed clarification work through `2f417072`, plus the local
  session-handshake correction in this checkpoint.
- Rust verification checkout: `/tmp/swarm-clarification-check.LS0pgn` on bgsdev.
- Separate native Hive: `/tmp/swarm-clarification-native.a3cefM`, API port8874,
  own database, socket, terminal history, agents and generated operator credential.
- Owned transient units: `swarm-clarification-native-web.service` and
  `swarm-clarification-native-host.service`. Initial API-only unit was stopped
  before the web-serving unit replaced it. Neither is a production service.
- Browser: separate Edge tab via loopback SSH forward. Debug binaries report
  `1.6.0`; that string is not evidence of a release or a production deployment.
- Native provider observed: Claude Code2.1.267. Only fictional prompts, no tasks,
  file changes, customer data, external messages or real project work requested.

## Passed: actual Queen question -> browser clarification -> MCP reply

Queen `01a089a6-9e40-7772-817e-49de3d0db35c` created the fictional no-task Input
decision `01a089aa-fa08-7da1-8203-79141139a657` using her native MCP tools.
The browser displayed it in Needs You with its original final-answer controls.

The first send exposed a genuine localhost integration defect: GET session
accepted trusted local access without creating a cookie, whereas clarification
correctly required a credential. The API returned401, saved no question, and the
browser retained the exact draft with an explicit error.

Correction: the already-trusted local session restore issues the same HttpOnly
browser credential as the existing POST-session path. The clarification command
still requires a credential. Relayed/public and foreign-origin requests do not
gain local trust; unconfigured local browsing is not turned into a startup error.
All34 matching authentication tests and strict API all-target Clippy passed.

After updating only the isolated API and restoring the browser session:

- Browser question: “Why is sample A safe, and does asking this question approve
  any work?”
- Round `fe0cda54-0570-4120-b49b-7f9502d63b1d`: asked1789016613,
  replied1789016619, delivery state `delivered`.
- Reply attributed to that Queen and session
  `01a089a6-9ed7-7033-95fb-507ecc45ab4f`, unchanged across the isolated API restart.
- Queen explained that the sample was fictional, no work was approved, and the
  decision remained pending. Persistence independently confirmed `pending` and
  clarification next move `operator`; prose was not treated as proof.
- The reply appeared in the real Inbox, not a fixture callback. Refresh retained
  the reply; “Read reply” retrieved it without resending the question.
- Dark390x844: measured scrollWidth390, readable wrapped explanation and final
  controls. Keyboard selection and submission worked. Viewport reset afterward.
- Only the explicit “Keep waiting” selection followed by “Send answers” resolved
  the original decision. Needs You returned to zero; clarification next move
  became `none`. The task API returned zero tasks and the same native session
  was resting. No command grant or task execution was requested.

The full web suite passed1512 tests in157 files in55.08 seconds. Some browser
click observations timed out; state was reread before retrying, and the final
answer was selected/submitted by keyboard. This is desktop/viewport evidence,
not a real Android/iOS keyboard or background-resume claim.

## Passed: sleeping ordinary author

Worker `01a089b8-421d-7f41-995e-82fa81b707bf`, autostart false, created decision
`01a089b9-cef5-7c53-a18a-b10e5032340c` through its native tools. After its provider
was resting with no background work, the controller left the terminal and stopped
only this fictional worker through the normal session-delete API. The API proved
running false, active_session_id null, and autostart false before asking.

The browser then asked for an explanation without a separate wake action.
Round `c1eff0a8-6cb0-493a-8a46-a06dafaa5020` was asked1789017204 and replied1789017218;
delivery was `delivered`, attributed to this requester and new session
`01a089bc-2cff-73c2-860a-7d7f91308fc0`. While waiting, the browser showed zero
operator actions and “1 conversation is waiting for a reply, not an answer from
you”; original options remained accessible. After the reply it showed one
operator action and the actual worker author. Queen's native session stayed
unchanged. This proves wake/delivery/reply, not every provider conversation-recovery
fallback. The controller then explicitly chose “Keep waiting” and sent answers.

## Remaining acceptance

Both fictional decisions were independently confirmed resolved with
`resolution_answers.Sample=["Keep waiting"]`. Both native providers were resting
without background work before their isolated API/host units were stopped. The
temporary Windows SSH forward (verified PID32720) was stopped too. The isolated
database/history remain as evidence; production services were not stopped.

The first combined API gate passed560, ignored3, and failed the existing
`api_recreation_reattaches_the_durable_queen_without_a_duplicate` fixture because
its `sleep 5` child expired before the final running assertion. The fixture now
uses an owned `cat` process stopped explicitly by the test (and killed by session
Drop on unwind), not a wall-clock lifetime. This is a test-only correction.
The repeated combined gate passed561 API tests (three opt-in tests ignored),
all48 application tests and all739 persistence tests. The final web production
build passed as well; its existing552-KB terminal-chunk warning remains.

- Current native tools work across the tested API replacement. An older client
  retaining a pre-clarification tool list is not proven by this; preserve ADR0053's
  normal provider refresh boundary and do not restart unrelated workers.
- Live development deployment and browser check passed; see the receipt below.
- Uncertain-delivery UI and failure fencing have component/persistence/API proof;
  this native happy-path scenario does not manufacture a real lost delivery.
- Direct terminal/AskUser answer correlation remains ADR0065 work, not solved by
  the new clarification exchange. The complete maturity goal remains open.

## Live development deployment receipt

Published and deployed `f05f7f217a7c43bf4b82105d8c4574a0e95223c3` through the
existing app/API-only development reload. Activation completed September 10 at
01:47:31 Eastern. Health reports
`1.6.0-dev-f05f7f217a7c-20260910054522-50600`, healthy with no degraded entries.
The pre-migration backup is
`~/.local/state/swarm/backups/pre-v158-reload-f05f7f217a7c-20260910T054522Z.sqlite3`
(45,543,424 bytes). No release was cut.

Private receipt directory `/tmp/swarm-clarification-deploy.cvm5VI` contains the
immediate pre/post worker snapshots: all34 records and all12 running provider/
session identities match exactly. Engine PID3408834 and its September9 17:01:49
Eastern start remain unchanged. The health engine-build fingerprint describes
the new candidate; the UI explicitly shows an unapplied engine update. That
update was not applied. These projections do not prove provider conversation IDs.

The separate Edge tab at `https://swarm.bfgsolutions.net` loaded the new UI using
its existing session. Both real pending decisions retain their choices and now
offer Ask a question. Opening and collapsing the empty clarification editor did
not submit anything or resolve either decision. No live operator answer was sent.
390x844 reported scrollWidth390; viewport restored. Physical-device acceptance
remains separate. CI34442276338 passed all four jobs: web, linux-package,
rust-audit and Rust workspace/all-features verification.
