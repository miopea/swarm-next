# ADR 0079: Explicit experimental-provider admission

Status: Accepted implementation design under the approved PROV-01 scope;
implementation and live acceptance pending.

## Context

The builder-owned Night Watch allowlist is distinct from executable discovery.
The running engine currently reports only Claude Code and Codex availability.
Gemini, Grok and OpenCode have bare interactive adapters, not completed Swarm
integration. Settings preserves existing experimental bindings but offers no
explicit path to opt into one. Merely finding an executable cannot establish
authentication, tool support, recovery or unattended readiness.

## Decision

- The terminal host reports experimental executable availability using its own
  launch environment. It does not run provider commands to discover availability.
  The response has a bounded, named set of the three implemented alpha adapters;
  arbitrary executable names and filesystem paths are not accepted or exposed.
- The additional capability response is optional for compatibility. Omission by
  an older engine means unknown, not installed or absent. The API and UI must keep
  experimental creation unavailable until the running engine supplies evidence.
- Availability is not maturity. The domain's builder-owned promotion list remains
  the authority for Night Watch admission; neither a checkbox nor a successful
  launch changes it. No provider is promoted by this work.
- New experimental bindings require an explicit operator acknowledgement in the
  creation/provider-change command. The command defaults to not acknowledged.
  Settings shows the experimental choices only after deliberate opt-in and
  explains the missing integration, manual recovery and Night Watch restriction.
- Editing unrelated details on an existing experimental worker preserves that
  binding without requiring a provider change. No fallback changes the selection
  when availability disappears. Missing executable or unknown capability is an
  actionable refusal, not a reason to launch another provider.
- The same admission rule covers a temporary alternate-provider worker. It must
  preserve the existing explicit handoff workflow, not silently substitute a
  provider or claim that the bare adapter can consume Swarm tools.
- Business eligibility belongs in domain/application behavior; HTTP and UI adapt
  its result. Existing Night Watch gates on startup and delivery remain required.
  Opt-in does not bypass workspace, authentication or engagement protections.
- No new background collection, polling owner or persistent secret is introduced.
  The existing capability refresh owns discovery. This does not install a CLI or
  change provider configuration on the operator's machine.

## Compatibility ownership and removal

The API capability adapter owns the absent-field path while older independent
engines remain supported. Remove that path when the supported engine floor
requires experimental availability reporting. Until then, ordinary Claude/Codex
availability and existing workers must remain usable during rolling updates.

## Acceptance required before completion

1. Old and new engine response fixtures preserve unknown versus unavailable.
2. Creation, changed binding and alternate-provider commands reject missing
   acknowledgement; an unchanged experimental binding remains unchanged.
3. UI tests cover opt-in, unavailable/unknown discovery, failed submission and
   preservation of an explicitly chosen provider. No automatic selection occurs.
4. Night Watch still rejects experimental wake and automated delivery; ending
   Night Watch restores ordinary eligibility without changing provider identity.
5. A disposable installed-alpha launch demonstrates truthful limitations and
   manual recovery. Do not install or enable providers on a real worker merely
   to satisfy this gate. Record absent real-provider evidence as unverified.
6. All promotion journeys remain governed by `../48-provider-acceptance.md`;
   this admission work is not evidence that those journeys passed.
