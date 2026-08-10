# Quality and acceptance

Status: **Proposed**

## Initial product targets

Targets are refined after legacy baseline measurements.

| Measure | Initial target |
|---|---|
| Visible worker switch | No terminal reconnect; perceptually immediate |
| Reload to synchronized selected terminal | Under 1 second locally at normal history size |
| Missing or duplicate terminal output | Zero in deterministic and fault tests |
| Worker loss during application update | Zero |
| Input delivered to stale replacement session | Structurally impossible |
| Idle memory trend | No sustained growth after warm-up |
| Queue and buffer bounds | 100% declared and observable |
| Recovery requiring hard refresh | Zero in supported scenarios |
| Idle CPU | Near zero after warm-up |
| Diagnostic subsystem attribution | Browser, API, terminal, provider, DB, or integration identified |

## Required test layers

- Domain state-transition tests.
- Property tests for invariants and terminal synchronization.
- Database migration, transaction, backup, and restore tests.
- Contract tests for HTTP, events, and terminal protocol.
- Recorded PTY stream tests, including alternate-screen applications.
- React component and state-machine tests.
- Browser journey tests.
- Crash, restart, reconnect, and stale-client tests.
- Slow-consumer and resource-exhaustion tests.
- Security boundary and authorization tests.
- Linux/WSL platform tests; native Windows only when explicitly targeted.
- 24–72-hour soak tests before promotion to daily-driver status.

## High-value stress scenarios

- Switch among 20 noisy workers 1,000 times.
- Reload during continuous output and during alternate-screen mode.
- Resize while output, switching, and reconnect occur.
- Suspend a browser and resume after the delta journal has and has not expired.
- Restart API and terminal runtime independently.
- Replace a worker process while a stale browser remains connected.
- Fill every queue and confirm its declared overflow behavior.
- Interrupt migration, backup, update, and rollback at each durable step.

## Definition of done

A feature is not done when its success path renders. It is done when:

- the user outcome and non-goals are documented;
- domain ownership is clear;
- failure, cancellation, restart, and recovery behavior are specified;
- resource bounds and security impact are reviewed;
- observability identifies its failures;
- automated tests cover material invariants;
- the primary operator has successfully dogfooded it at the appropriate ring.

