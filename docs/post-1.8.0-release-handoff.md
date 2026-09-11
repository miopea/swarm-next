# Post-1.8.0 development package handoff

This is a reviewed change summary, not a published release or full maturity signoff.
Do not publish without the operator's current release authority.

## Exact comparison

- Existing public release: v1.8.0, published September11 at11:54:06 UTC.
- Release tag commit:402d754d68c2ae449fa0a215160e330e64f6e42e.
- Tested development source:09bc29cc0c5480aead8f57b43f1363839829e8d2.
- Production dev and WSL:1.7.1-dev-09bc29cc0c54-20260911144639-1439784.
  The development version prefix is not a new public release version.
- Later commits contain handoff evidence and an isolated browser-fixture correction,
  not additional shipped runtime behavior.
- Full CI gate34612113986: SUCCESS. Web, Rust audit, Linux package and Rust all
  passed; the final Rust job completed in14m58s. The development candidate is
  ready for the release worker's final review, not automatically published.

## Suggested What's New card

**A steadier Hive, with fewer lost edits**

- Keep unfinished worker setup when moving between Settings sections or searching.
- Keep custom interview answers when switching between Needs You and Activity.
- Refresh Apiary details without reloading the page or losing management edits.
- See genuine worker problems without false alarms from the API's protected filesystem.
- Request a decision without inventing a recommendation when there is no preference.

## Additional fixes

- Cancel obsolete Apiary management reads and let stalled reads time out and retry.
- Reconcile an uncertain cross-Hive handoff without losing its draft.
- Reject partial or replaced worker-recovery observations rather than treating them
  as complete evidence.

These are fixes after1.8.0. Optional Jira, one-approval joining and the Apiary home
already shipped; do not announce those again as new in this package.

## Verification and release boundaries

The latest update preserved both worker-engine processes and all eight exact
production worker/session pairs. It did not activate the pending worker engine.
137 focused Settings/App tests, TypeScript and both development builds passed.
Edge verified unsaved name/path retention on deployed WSL; fictional390px queue
and long-decision interactions passed. No live tasks or decisions were changed.

Use the normal signed release process; do not rename the development archive into
a public artifact. Native answer reconciliation, safe automatic engine admission,
real-device camera/gallery/handoff, sustained performance and the other full-scope
gates remain open. Detailed evidence and current limits are in
[the maturity closeout](maturity-user-facing-closeout.md).
