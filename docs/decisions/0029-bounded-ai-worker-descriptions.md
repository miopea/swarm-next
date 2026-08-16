# ADR 0029: Bounded AI-assisted worker descriptions

Status: **Accepted**

## Context

Queen routes work more reliably when each durable worker has a concise
operator-reviewed description of its repository ownership. Hand-writing every
description creates setup friction, while giving a model general repository
access for this small task would expose more data and capability than the
outcome requires.

## Decision

Swarm keeps the existing deterministic local draft and adds an explicit
**Improve with Claude** action. Swarm itself constructs the only model input:
repository name, deterministic draft, manifest description, and the first
usable README paragraph. It never sends source files, credentials, tasks,
terminal output, provider conversations, or arbitrary repository contents.

The Claude subprocess runs in an isolated temporary directory with:

- all tools disabled and MCP restricted to an explicit empty configuration;
- project-only settings loaded from the empty temporary directory;
- no session persistence;
- one turn, a 45-second timeout, and a $0.10 maximum budget;
- bounded input, stdout, and stderr capture; and
- a strict JSON schema containing only the description.

Only one improvement may run per Swarm API process. The returned description
replaces only the editable form value. The operator must still choose Save.
The free local draft remains available when Claude is absent, times out, or
returns an invalid result.

## Consequences

- Worker setup gains useful language assistance without creating a repository
  browsing agent or durable model session.
- The operator sees the cost and privacy boundary before invoking Claude.
- Failure is contained to the unsaved draft and never changes worker identity,
  routing, or provider state.

## Validation

Unit tests prove metadata exclusion, strict structured parsing, tool-free
single-turn arguments, and a complete subprocess round trip through a fake
Claude executable. Browser tests prove both draft paths remain editable and
unsaved until the operator saves.
