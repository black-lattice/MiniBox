# MiniBox TODO

## P0: Planning Baseline

- [ ] Review and approve updated `PRODUCT.md`
- [ ] Review and approve updated `ARCHITECTURE.md`
- [ ] Review and approve updated `MODULES.md`
- [ ] Review and approve updated `README.md`
- [ ] Confirm the internal config model fields required for level-B Clash support
- [ ] Confirm cache file format and local storage expectations on macOS and Linux

## P1: Project Skeleton

- [x] Add `src/lib.rs` and module skeletons matching the planning docs
- [ ] Keep the crate buildable while modules remain placeholder-only
- [x] Add crate conventions for errors, logging, and metrics surfaces
- [ ] Add linting and formatting configuration
- [ ] Add CI for build, test, fmt, and clippy

## P2: Internal Config Model First

- [ ] Implement `config::internal` types for listeners, nodes, groups, limits, and active snapshots
- [ ] Implement strict validation rules for internal references and invariants
- [ ] Define the boundary contract from `config::external` into `config::internal`
- [ ] Add tests for normalization and validation

## P3: Listener Framework

- [ ] Implement listener lifecycle and accept-loop framework
- [ ] Wire listener selection to the active internal config snapshot
- [ ] Define runtime connection accounting and admission control

## P4: Protocols

- [ ] Implement SOCKS5 protocol handling
- [x] Implement HTTP CONNECT protocol handling
- [ ] Add protocol-focused tests for negotiation and failure paths

## P5: Relay Pipeline

- [ ] Implement upstream dialing from internal node definitions
- [ ] Implement bounded bidirectional TCP relay
- [ ] Add shutdown-aware session lifecycle handling
- [ ] Add relay tests for timeout, close ordering, and byte accounting

## P6: Metrics and Logging

- [x] Add typed descriptors for logging events, metrics, and health/readiness probes
- [ ] Implement structured logging initialization
- [ ] Implement low-cardinality runtime metrics
- [ ] Add health and readiness surfaces
- [ ] Document operational event taxonomy

## P7: Clash Subscription Adapter

- [ ] Implement supported Clash external schema parsing
- [ ] Translate nodes into the internal config model
- [ ] Translate supported groups into the internal config model
- [ ] Reject unsupported rule-level features clearly
- [ ] Add translation diagnostics and tests

## P8: Provider Cache and Rollback

- [ ] Implement last-known-good cache persistence
- [ ] Load cache during startup when external ingestion fails
- [ ] Prevent invalid translations from replacing the cache
- [ ] Add tests for cache fallback and rollback behavior

## Later Phases

- [ ] Evaluate safe refresh or reload flows after the snapshot model is stable
- [ ] Evaluate optional TLS-related outbound features if they fit the architecture
- [ ] Evaluate more adapters only if they preserve the same translation boundary
- [ ] Reassess whether the module plan still fits a single crate
