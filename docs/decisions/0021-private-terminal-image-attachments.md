# ADR 0021: Private bounded terminal image attachments

Status: **Accepted**

## Context

Operators use screenshots heavily while steering coding workers, especially on
mobile. A remote browser cannot place clipboard image bytes directly into a
server-owned PTY, and pretending that terminal text paste can carry an image
produces silent failure. Terminal output, the application database, and public
web assets are also inappropriate attachment stores.

## Decision

An authenticated, session-scoped API accepts PNG, JPEG, WebP, and GIF clipboard
images after validating both the declared media type and file signature. The
API stores them in a private same-user state directory and returns only the
absolute local path. The browser inserts that path into the selected worker's
input using bracketed paste but never submits it automatically.

- One image is limited to 8 MiB.
- The attachment store is limited to 128 MiB and 64 retained files.
- Files older than seven days are pruned before admitting a new image.
- Capacity exhaustion rejects the upload; it never grows without a bound.
- Filenames are content-derived and contain no operator or workspace text.
- Image bytes, paths, and hashes never enter logs, SQLite, activity events, or
  feedback diagnostics.
- The endpoint requires the existing operator credential and a valid terminal
  session identity.

## Consequences

Ctrl+V can carry screenshots from desktop or mobile to a worker without making
the browser or database own an unbounded binary history. Attachments remain a
local Hive implementation detail and can later move behind a provider-native
file-input adapter without changing the product interaction.

## Validation

- API/storage tests cover type/signature rejection, size bounds, private file
  permissions, and a successful content-addressed write. The endpoint uses the
  same mandatory operator authorization boundary as terminal attach.
- Browser tests cover leaving ordinary text paste untouched, clipboard-image
  classification, path insertion, and the no-auto-submit rule. An isolated
  browser/API pass verifies the authenticated upload contract end to end.
