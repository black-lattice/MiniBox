# wanglin-proxy Module Plan

## Crate Layout

The project should remain a single crate while the core boundaries settle. Internal modules should mirror the planning model instead of chasing immediate implementation convenience.

Proposed layout:

- `src/main.rs`
- `src/lib.rs`
- `src/bootstrap.rs`
- `src/error.rs`
- `src/runtime.rs`
- `src/listener.rs`
- `src/relay.rs`
- `src/metrics.rs`
- `src/logging.rs`
- `src/subscription.rs`
- `src/adapter/mod.rs`
- `src/adapter/clash.rs`
- `src/provider/mod.rs`
- `src/provider/cache.rs`
- `src/protocol/mod.rs`
- `src/protocol/socks5.rs`
- `src/protocol/http_connect.rs`
- `src/config/mod.rs`
- `src/config/internal.rs`
- `src/config/external.rs`

## Layer Mapping

### Core Proxy Runtime Layer

Modules:

- `bootstrap`
- `runtime`
- `listener`
- `protocol/*`
- `relay`
- `metrics`
- `logging`

Rules:

- may depend on `config::internal`
- must not depend on adapter-specific external schema types
- must not parse Clash subscription structures directly

### Internal Config Model Layer

Modules:

- `config::internal`

Rules:

- defines canonical runtime-facing types
- should be usable by tests and runtime without adapter code
- should not depend on `adapter`, `provider`, or protocol implementation details

### External Adapter Layer

Modules:

- `config::external`
- `adapter::clash`
- `subscription`
- `provider::cache`

Rules:

- may parse and translate external schemas
- may produce diagnostics about unsupported features
- may depend on `config::internal` as a translation target
- must not add runtime-only branches into the core modules

## Module Responsibilities

### `main`

Responsibility:

- minimal binary entrypoint
- call into the library and present current implementation status

### `lib`

Responsibility:

- define the module tree
- expose stable placeholder APIs for iterative implementation

### `bootstrap`

Responsibility:

- orchestrate startup in plan order
- load startup sources
- choose the active validated internal config snapshot

Planned interface:

- `build_startup_plan() -> StartupPlan`
- later `run(...) -> Result<(), Error>`

### `runtime`

Responsibility:

- hold runtime-safe active config state
- expose immutable access to listeners, nodes, groups, and limits

Key constraint:

- runtime state consumes only `config::internal` types

### `listener`

Responsibility:

- define the listener framework skeleton
- describe how listeners attach protocol handlers and runtime state

Key constraint:

- no external config parsing here

### `relay`

Responsibility:

- define the relay pipeline contract for bidirectional TCP forwarding

Key constraint:

- pure runtime concern; no provider or adapter behavior

### `metrics`

Responsibility:

- placeholder for runtime and adapter-visible counters
- keep metric surfaces intentionally low-cardinality

### `logging`

Responsibility:

- central place for logging initialization and event taxonomy planning

### `subscription`

Responsibility:

- define subscription source abstractions and update intents
- coordinate provider, adapter, validation, and activation flow at a high level

Key constraint:

- orchestration only; translation details belong in adapters

### `adapter::clash`

Responsibility:

- parse the supported Clash subset
- translate nodes and groups into `config::internal`
- document unsupported rule features explicitly

Support boundary:

- level B only: nodes plus groups
- no full rules compatibility

### `provider::cache`

Responsibility:

- persist and load last-known-good translated snapshots
- support rollback selection on startup or refresh failure

Key constraint:

- cache stores validated internal snapshots or a serialized equivalent, not raw runtime state

### `protocol::socks5`

Responsibility:

- define the SOCKS5 listener/handshake skeleton

### `protocol::http_connect`

Responsibility:

- define the HTTP CONNECT listener/handshake skeleton

### `config::internal`

Responsibility:

- define canonical runtime-facing types such as listeners, nodes, groups, limits, and active snapshots

Key constraint:

- this is the runtime contract

### `config::external`

Responsibility:

- define external-source wrappers and translation boundary types
- keep raw external semantics isolated from the runtime

Key constraint:

- external config types stop here or in adapters and must be translated before runtime use

### `error`

Responsibility:

- define crate-level placeholder error types
- keep error boundaries explicit from the start

## Dependency Direction

Preferred dependency flow:

- `main` -> `lib` modules
- `bootstrap` -> `subscription`, `provider::cache`, `config::internal`, `runtime`, `listener`, `metrics`, `logging`
- `listener` -> `protocol::*`, `runtime`
- `protocol::*` -> `config::internal`, `runtime`
- `subscription` -> `adapter::clash`, `provider::cache`, `config::external`, `config::internal`
- `adapter::clash` -> `config::external`, `config::internal`
- `provider::cache` -> `config::internal`

Rules:

- core runtime modules do not depend on `adapter::clash`
- `config::internal` does not depend on any runtime or adapter module
- `config::external` is never consumed directly by runtime modules

## Translation Boundary

The project needs an explicit line between external and internal config.

Boundary rule:

- external source -> `config::external` / adapter-specific parsing -> `config::internal::ActiveConfig`

Anything not representable in `config::internal` is unsupported for the MVP. That keeps implementation honest and prevents silent pseudo-compatibility.
