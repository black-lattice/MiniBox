# MiniBox Architecture

## Overview

`MiniBox` is a single-process Rust service with a narrow, planning-first architecture. The main design constraint is that external compatibility work must not distort the proxy core.

The architecture is intentionally divided into three layers:

1. core proxy runtime
2. internal config model
3. external adapters, including Clash subscription ingestion

This order matters. The runtime serves traffic. The internal model defines what the runtime can understand. Adapters translate external inputs into that model and stop there.

## Design Principles

- keep the proxy data path short and bounded
- make the internal config model the only config shape the runtime consumes
- isolate external compatibility logic behind adapters
- treat imported subscription data as replaceable input, not native semantics
- prefer explicit failure and rollback over silent partial updates

## Three-Layer Model

### Layer 1: Core Proxy Runtime

Responsibilities:

- bootstrap process state
- bind listeners
- accept downstream connections
- perform protocol handshakes
- dial upstream nodes selected from the internal config
- relay traffic with bounded buffers
- expose metrics, logs, and health signals

Allowed knowledge:

- runtime-safe internal types
- validated listener, node, and group definitions
- cache state only as an opaque active snapshot chosen before serving

Forbidden knowledge:

- raw Clash schema types
- provider-specific parsing details
- adapter-specific compatibility flags

### Layer 2: Internal Config Model

Responsibilities:

- define normalized runtime-facing listeners, nodes, groups, and limits
- validate invariants needed by the runtime
- represent exactly the features that `MiniBox` supports
- serve as the translation target for external config sources

Properties:

- stable and testable
- smaller than any external format it imports
- independent of protocol parser or provider quirks

This layer is the contract between planning and implementation. If an external format contains more features than the model supports, the adapter must reject or drop them explicitly according to documented rules.

### Layer 3: Adapter and Clash Subscription Ingestion

Responsibilities:

- fetch or read external subscription payloads
- parse external schemas
- translate supported content into the internal config model
- manage provider cache persistence for last-known-good rollback

Supported external work in the MVP:

- Clash subscription ingestion at level B
- node extraction
- group extraction
- translation into internal nodes and groups

Deferred external work:

- full rule compatibility
- scripts and provider variants beyond the documented subset
- any adapter feature that requires runtime changes for unsupported semantics

## Why Adapters Must Not Pollute the Core

Clash compatibility pressure can easily spread external assumptions through the entire codebase. That would create a runtime that is harder to reason about and harder to stabilize.

Adapter isolation exists to prevent:

- raw Clash fields leaking into listener and relay logic
- runtime branches based on external source format
- future adapter additions forcing changes in the hot path
- false compatibility claims caused by partial support hidden inside the core

The core should only know that it received a validated internal snapshot. It should not care whether that snapshot came from a local static file, a Clash subscription, or another adapter added later.

## Runtime Architecture

Core runtime pieces:

- bootstrap layer loads startup inputs and chooses the active config snapshot
- listener framework binds one or more local listeners
- protocol handlers manage SOCKS5 and HTTP CONNECT negotiation
- session runtime dials upstream nodes and coordinates relay
- relay pipeline performs bounded bidirectional copy
- metrics and logging surface lifecycle and failure signals

Startup flow:

1. load local startup settings
2. load or translate external inputs into the internal config model
3. validate the resulting internal snapshot
4. choose active snapshot, including cache fallback if needed
5. initialize logging, metrics, and shutdown coordination
6. bind listeners and begin serving

Connection flow:

1. listener accepts downstream socket
2. protocol layer performs SOCKS5 or HTTP CONNECT negotiation
3. runtime selects an upstream node or group target from the active internal snapshot
4. upstream connection is established
5. relay pipeline copies bytes in both directions using bounded buffers
6. metrics and logs record outcome

## Internal Config Model

The internal model is the canonical runtime input. It should define only concepts that the runtime can actually execute.

Likely concepts:

- process settings
- listener definitions
- protocol mode
- outbound nodes
- groups referencing supported node choices
- provider cache metadata
- timeout and buffer limits

Validation rules should enforce:

- listeners reference supported protocol handlers
- groups reference existing nodes or supported child groups
- unsupported external fields never reach the runtime
- unsafe or ambiguous configuration fails before listeners bind

## Adapter and Provider Pipeline

The subscription path should be explicit rather than magical.

Recommended ingestion steps:

1. obtain raw provider content
2. parse external schema in adapter-specific code
3. translate into internal config structures
4. validate translated snapshot
5. write cache candidate only after validation succeeds
6. atomically switch active snapshot to the validated result

The provider or adapter layer may expose diagnostics about translation loss, unsupported fields, and rejected features, but those details stop at the boundary.

## Cache and Rollback Behavior

Clash subscription support introduces failure modes that the static local config path does not have. The system therefore needs explicit cache and rollback semantics.

Expected behavior:

- keep a local persisted last-known-good translated snapshot
- never overwrite that cache with an invalid translation result
- on startup or refresh failure, prefer the last-known-good snapshot if available
- if no valid cache exists, fail clearly rather than serving an unknown partial state

Why this matters:

- remote or generated subscription payloads can break unexpectedly
- unsupported fields may appear after a provider-side change
- network fetches can fail or return incomplete data

Operational rule:

- rollback is a provider-layer concern that decides which validated internal snapshot becomes active
- rollback must not require special branches inside protocol handlers, relays, or upstream dialing code

## Concurrency Model

The MVP should use a single async runtime per process.

Rules:

- one accept loop task per listener
- one session task per downstream connection
- bounded per-session buffers in the relay path
- no unbounded channels in the hot path
- config snapshots are shared immutably once activated

This fits the three-layer model well because translation and cache decisions happen before the snapshot becomes active in the runtime.

## Observability

The system should expose enough signals to debug both proxy failures and subscription failures.

Core runtime signals:

- listener start and stop
- session start and end
- upstream dial failures
- relay termination reasons

Adapter and provider signals:

- subscription fetch attempts
- translation success and failure
- cache load success and failure
- rollback activation events

Metrics should keep label cardinality low and avoid embedding raw subscription metadata or provider-specific identifiers in hot-path metrics.

## Security and Trust Boundaries

Trusted inputs:

- local operator configuration
- local cache files produced by validated translations

Less trusted inputs:

- remote subscription payloads
- provider content with changing schemas

Security posture:

- external payloads are parsed and validated outside the core runtime
- unsupported or ambiguous features fail closed
- logs should avoid leaking secrets or entire subscription payloads

## Major Tradeoffs

- The architecture favors clean boundaries over broad compatibility claims.
- Clash support is intentionally partial so the runtime can remain small and stable.
- Cache rollback is included early because external subscriptions make failure recovery part of core operations, even if adapter logic stays outside the hot path.
