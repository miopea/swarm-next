# Operator-managed repository search folders

Status: Accepted — operator requested editable workspace paths and repository search.

The Workers settings page owns additional trusted repository search folders for
this Hive. Installation roots remain visible and immutable here. Adding a folder
is explicit operator approval to discover/select repositories within it; it does
not move files, retarget existing workers, grant Keeper filesystem authority, or
restart workers. Removing a search folder does not remove its workers or files.

Persist at most16 canonical additional folders, each at most4096 bytes, with a
revision so stale editors cannot silently overwrite one another. The application
owns limits and replacement behavior, persistence owns the transaction, and the
filesystem adapter validates real directories and canonicalizes before saving.
Reject filesystem roots and symbolic-link endpoints. `~/` refers to this Hive's
home, never the browser device. No shell expansion is performed.

Discovery remains bounded to256 results/depth6. Additional roots participate in
worker creation validation; the independent host still receives the existing
explicit outside-installation-root flag for persisted worker paths. No host
configuration mutation or restart is needed. A missing search folder must report
which folder is unavailable and remain editable rather than blocking recovery.

Acceptance: save/reopen, stale revision refusal, invalid-path refusal without
partial replacement, add/remove discovery, existing worker continuity, and a
browser check at desktop/mobile widths. This does not authorize release.
