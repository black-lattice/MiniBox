# MiniBox

`MiniBox` is a planning-first Rust proxy aimed at self-hosters and power users who want a lightweight daily-use proxy with explicit boundaries and low operational overhead.

The repository now contains a minimal direct TCP proxy data path for SOCKS5 and HTTP CONNECT, plus startup/config plumbing for local files and level-B Clash subscription translation. It is still intentionally narrow in scope and is not feature-complete.

## Scope

Near-term product direction:

- local daily-use proxy runtime for macOS and Linux
- SOCKS5 and HTTP CONNECT listener modes
- bounded TCP relay core
- Clash subscription support at level B only: nodes plus groups
- local cache rollback for last-known-good translated subscription state

Explicitly out of scope for the MVP:

- full Clash rule compatibility
- TUN mode or transparent proxying
- Windows support
- dynamic control-plane style management

## Planning Documents

- [PRODUCT.md](./PRODUCT.md): target users, deployment targets, support boundaries, and release criteria
- [ARCHITECTURE.md](./ARCHITECTURE.md): three-layer architecture, cache/rollback behavior, and adapter isolation rules
- [MODULES.md](./MODULES.md): planned module layout and translation boundaries
- [TODO.md](./TODO.md): implementation order and current priorities

## Current Status

- docs updated for level-B Clash subscription support
- minimal direct SOCKS5 and HTTP CONNECT runtime path present
- initial structured logging, metrics, and health/readiness planning hooks present
- `cargo test` should succeed as a baseline integrity check

## Implementation Order

1. internal config model
2. listener framework
3. SOCKS5
4. HTTP CONNECT
5. relay pipeline
6. operations surfaces: logging, metrics, and probes
7. Clash subscription adapter
8. provider cache and rollback hardening

The architecture is intentionally strict: the runtime consumes only the internal config model, while Clash subscription support stays in adapter and provider layers.
