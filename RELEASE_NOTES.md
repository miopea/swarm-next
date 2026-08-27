# Release notes

What each release means, written for the people running Swarm rather than for
the people who wrote it.

`swarm-release notes` prefers a section here over the commit subjects, per
release. A version with no section falls back to generated notes, and the build
says on stderr which source each release used — so notes written for a version
that never gets cut are noticed rather than silently dropped.

Format: `## <version>`, then `### New features` and `### Fixes`, then `- ` bullets.
End a bullet with `(after the worker engine update)` when it is installed but
not in effect until the worker engine swaps.

## 0.8.19

### New features
- The What's New panel now shows everything since the release you actually
  updated from, so opening the control room on a second machine no longer
  looks like a first install

### Fixes
- A Hive with Outlook registered and no public address starts again, instead of
  exiting on boot and being restart-looped
- A failed update can be recovered from with ordinary commands. The pre-update
  backup no longer needs the API it is protecting, a rollback restores your
  database rather than leaving it ahead of the release, and a service that has
  been crash-looping is reset instead of refusing every later install
- Update checks report the engine your Hive is running, not the one it was
  installed from
