# ADR 0026: Opt-in same-port development reload

Status: **Accepted for the dogfood host**

## Context

The primary operator develops Swarm Next from inside Swarm Next. A normal
immutable release is the right production boundary, but packaging every local
iteration by hand makes dogfooding unnecessarily slow. Running a second dev
server on another port would exercise a different route, cookie, PWA, and
terminal attachment path than the installed application.

Development convenience must not reconnect the API to PTY ownership, execute
arbitrary browser-provided commands, or replace a healthy build with a failed
compile.

## Decision

An explicit package command connects one Git working tree below the operator's
home directory to the installed application. It enables a systemd user path
unit and an authenticated, content-free API trigger. The browser can request a
reload but cannot choose a checkout, command, environment variable, or output
path.

The path service builds the configured working tree into a uniquely versioned,
checksummed development package. Dirty tracked and untracked source files are
allowed because that is the purpose of this mode. The existing package updater
activates the build only after compilation succeeds, verifies protocol
compatibility, restarts only the API on `127.0.0.1:8766`, checks health, and
rolls back on failure. The independently pinned terminal host, PTYs, provider
processes, database, and repositories remain untouched.

The browser polls the public health version while the old API continues to
serve. It reloads only after a different healthy version appears. Development
mode is visibly labeled in Settings, requires a confirmation for each build,
and can be disabled without changing the active release.

Every packaged API also carries the exact 12-character source revision whose
product tree it represents. This is distinct from the packaging commit when a
host-compatibility branch builds the release. Development reload is offered
only when that source is an ancestor of the configured checkout. An older or
unrelated checkout is shown as a blocked source mismatch instead of being
built. Failed-build status is revision-bound, so a stale or legacy failure
marker cannot keep a later healthy source in a failed state.

## Consequences

- Local iteration uses the exact installed URL, authentication, PWA, and
  terminal recovery path that the operator dogfoods.
- A compile or web-build failure leaves the current application running.
- A compatibility deployment cannot make an older checkout appear newer, and
  a stale failed-build marker cannot survive a source change.
- Enabling development mode is a host-side administrative action, not a web
  capability; the web trigger is inert when no checkout is configured.
- The build service intentionally has access to the selected checkout and the
  user's package/state directories. It remains an unprivileged user service
  with `NoNewPrivileges`, a private temporary directory, and a read-only system
  tree.
- Protocol changes still require the existing explicit zero-session migration;
  a development reload never kills active workers to make itself succeed.

## Validation

- API tests prove authentication, explicit availability, no-store status, a
  content-free request, fail-closed behavior when disabled, source ancestry,
  and revision-bound failure reporting.
- The package lifecycle smoke proves enable/disable configuration and confirms
  that neither action restarts the terminal host.
- The real dogfood proof must build a working-copy package, change the API
  version on port `8766`, preserve terminal-host PID and session IDs, and pass
  desktop and Android-sized browser checks. Every proof tab is closed
  immediately afterward.
