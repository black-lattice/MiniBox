# MiniBox Product Definition

## Purpose

`MiniBox` is a lightweight Rust proxy for self-hosters and power users who want a small, understandable daily-use proxy binary rather than a large integrated platform.

The project remains planning-first. The implementation should grow from a narrow core with explicit boundaries, stable operations, and low memory overhead.

## Product Position

`MiniBox` is intended to sit between:

- full-featured desktop proxy stacks with broad compatibility but high complexity
- ad hoc local proxy scripts and one-off tunnels that are hard to operate reliably

The target outcome is a practical personal or small-host proxy service that is easy to deploy on a laptop, mini PC, home server, or Linux box and predictable enough to leave running every day.

## Goals

- Provide a single-binary proxy runtime with bounded resource usage.
- Support local daily-use proxy scenarios for technically capable operators.
- Keep the core runtime independent from any one external subscription format.
- Support import of Clash subscriptions at level B: nodes plus groups, without claiming full Clash rule compatibility.
- Make failure and rollback behavior explicit when subscription ingestion goes wrong.
- Preserve a codebase structure that can be implemented incrementally and reviewed easily.

## Non-Goals

- Not a full Clash-compatible replacement.
- Not a general rule engine in the MVP.
- Not a GUI product.
- Not a plugin platform.
- Not a cloud control plane or hosted subscription manager.
- Not a Kubernetes-focused system.
- Not a cross-platform parity target for Windows in the near term.

## Target Users

- self-hosters running proxy services on their own laptops, mini PCs, home servers, or rented boxes
- power users who want a lightweight proxy for daily browsing, tooling, or selective tunnel use
- operators who need a maintainable Rust binary instead of a feature-heavy desktop proxy client

These users care about:

- small operational footprint
- easy local deployment
- understandable config behavior
- safe fallback when imported subscriptions are invalid

## Deployment Targets

Primary deployment targets:

- Linux `x86_64` and `aarch64`
- macOS on Apple Silicon and Intel
- home Linux boxes, mini PCs, NAS-adjacent hosts, and always-on personal servers
- local bare-metal or systemd-managed host deployments
- containerized deployments where desired

Out of scope for the MVP:

- Windows support
- Kubernetes-specific controllers or CRDs
- clustered multi-process coordination

## MVP Scope

The first usable release should include:

- static startup configuration from a local file
- a validated internal config model that the runtime consumes
- one or more local listeners
- SOCKS5 and HTTP CONNECT listener modes
- bounded relay behavior for TCP proxying
- structured logging and basic metrics
- subscription ingestion pipeline hooks with local cache support
- Clash subscription import at level B:
  - node parsing
  - proxy group parsing
  - translation into the internal config model
  - last-known-good cache rollback if import fails

The MVP should avoid:

- full Clash rule compatibility
- transparent proxying or TUN/TAP modes
- remote provider update daemons with complex scheduling
- dynamic admin mutation APIs
- broad protocol support beyond the explicitly planned listener modes

## Clash Subscription Support Boundary

Supported at level B:

- ingest a Clash-style subscription or provider payload
- extract proxy nodes
- extract proxy groups that can be mapped to internal group semantics
- translate supported data into the internal config model used by the runtime
- persist a local cache of the last known good translated result

Not supported in the MVP:

- full Clash rules engine
- script rules, rule providers, or premium-only features
- full field-for-field compatibility promises
- behavior identical to Clash implementations

The product promise is narrower: imported Clash data is treated as an external input format, not as the native runtime model.

## Success Metrics

Operational metrics:

- process RSS remains bounded and reasonable for a personal always-on proxy
- no unbounded queue or cache growth during steady state
- startup and shutdown remain deterministic on macOS and Linux
- invalid subscription updates do not replace a last-known-good cached config

Reliability metrics:

- zero known crash loops in soak testing under normal local-use workloads
- malformed external configs fail with actionable diagnostics
- rollback behavior is deterministic when cache is available

Maintainability metrics:

- core runtime code remains independent from Clash-specific data structures
- config translation boundaries stay explicit and testable
- module structure supports incremental protocol and adapter implementation

## Roadmap

### Phase 0: Design and Skeleton

- finalize scope for the lightweight daily-use proxy
- lock internal config model boundaries
- document adapter and cache rollback behavior
- scaffold the crate layout for iterative implementation

### Phase 1: Core Runtime MVP

- implement internal config model and validation
- implement listener framework
- implement SOCKS5 and HTTP CONNECT handling
- implement upstream dialing and bounded relay
- implement logging and metrics

### Phase 2: Clash Level-B Ingestion

- implement Clash subscription parsing and translation
- implement local provider cache and rollback
- validate node and group mapping semantics

### Phase 3: Hardening

- expand verification coverage
- improve operational diagnostics
- evaluate safe config reload or refresh flows only if they preserve the architecture boundaries

## MVP Release Criteria

- runs as a single documented binary on macOS and Linux
- supports basic local proxy use through SOCKS5 and HTTP CONNECT
- uses the internal config model as the only runtime-facing config representation
- imports supported Clash subscriptions with nodes and groups only
- preserves last-known-good local cache on failed imports
- documents unsupported Clash compatibility areas clearly
