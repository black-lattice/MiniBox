# MiniBox

`MiniBox` is a planning-first Rust proxy aimed at self-hosters and power users who want a lightweight daily-use proxy with explicit boundaries and low operational overhead.

The repository now contains a SOCKS5 and HTTP CONNECT local proxy runtime, Trojan outbound support, and Clash subscription startup plumbing. Subscription startup merges translated Clash content with the repository-local listener template in `config/example.yaml`, so the runtime can come up with a single `ActiveConfig`.

## Scope

Near-term product direction:

- local daily-use proxy runtime for macOS and Linux
- SOCKS5 and HTTP CONNECT listener modes
- bounded TCP relay core
- Clash subscription ingestion at the adapter boundary
- Trojan outbound execution for imported `trojan` nodes
- repository-local listener template merged into subscription startup
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
- [RELEASE.md](./RELEASE.md): short release checklist and support boundary recap

## Current Status

- docs updated for level-B Clash subscription support
- local SOCKS5 and HTTP CONNECT runtime path present
- imported Clash Trojan nodes now enter the internal config model and can execute through the runtime Trojan outbound path
- subscription startup composes translated Clash content with the local listener template
- initial structured logging, metrics, and health/readiness planning hooks present
- `cargo test` should succeed as a baseline integrity check

## Quick Start

Run the repository-local template:

```bash
cargo run
```

The default template in [config/example.yaml](./config/example.yaml) starts:

- SOCKS5 on `127.0.0.1:1080`
- HTTP CONNECT on `127.0.0.1:8080`

Run against a Clash subscription:

```bash
cargo run -- http://example.com/subscription
```

At startup, MiniBox emits structured log lines on `stderr` for:

- `startup.begin`: startup source and phase
- `startup.activated`: activated config source, cache rollback state, listener count, and admin bind state
- `admin.bound`: admin bind address and `/healthz` `/readyz` `/metrics` paths when admin is enabled
- `listener.bound`: each bound listener address and the target group/node it resolves to
- `runtime.readiness_changed`: readiness state transitions during startup and shutdown

If you run against a subscription, keep this behavior in mind:

- `event=startup.activated activated="fresh_translation"` means the subscription translated cleanly.
- `event=startup.activated activated="last_known_good_cache"` means the current subscription failed translation and the cached snapshot was used.
- `event=startup.activated cache_rollback="used"` means the process did not activate fresh subscription content.

The default template exposes two local entrypoints:

- SOCKS5 on `127.0.0.1:1080`
- HTTP CONNECT on `127.0.0.1:8080`

Point your browser, system proxy, or local client at one of those ports. If you only want a SOCKS5 path, the default template is sufficient. If you need HTTP CONNECT, use the `local-connect` listener from `config/example.yaml`.

## Admin Endpoints

MiniBox can expose a small local admin surface when `admin.enabled` is set to `true` in the active config.

Available paths:

- `/healthz`: liveness probe, returns `200`
- `/readyz`: readiness probe, returns `200` only after listeners are bound
- `/metrics`: Prometheus text exposition with low-cardinality runtime gauges

Minimal config shape:

```json
{
  "admin": {
    "enabled": true,
    "bind": "127.0.0.1:9090",
    "access_token": "secret-token"
  }
}
```

If `access_token` is set, send `Authorization: Bearer <token>` to access the admin endpoints.

## Deployment

The repository ships minimal deployment templates under `examples/`.

### systemd

Use [`examples/systemd/minibox.service`](./examples/systemd/minibox.service) as a starting point.

The sample assumes the binary is installed at `/usr/local/bin/minibox` and the startup source is stored in `/etc/minibox/source`. For a repository-local config file, point `ExecStart` at that path instead.

### launchd

Use [`examples/launchd/ai.minibox.plist`](./examples/launchd/ai.minibox.plist) as a starting point.

The sample assumes the binary is installed at `/usr/local/bin/minibox` and the startup source is stored in `/etc/minibox/source`. Replace the second `ProgramArguments` entry with a subscription URL or a local config file path.

### Troubleshooting

- If startup exits with `did not yield any listeners to serve`, the source did not provide a listener template. Use `config/example.yaml` or a subscription startup path that merges the template.
- If startup logs `event=startup.activated cache_rollback="used"`, the subscription translation failed and the last-known-good cache was activated.
- If a local client cannot connect, verify it points to `127.0.0.1:1080` for SOCKS5 or `127.0.0.1:8080` for HTTP CONNECT.
- If you only see direct traffic, confirm the active listener resolves to a `Trojan` node in the startup summary.

## Implementation Order

1. internal config model
2. listener framework
3. SOCKS5
4. HTTP CONNECT
5. relay pipeline
6. operations surfaces: logging, metrics, and probes
7. Clash subscription adapter
8. provider cache and rollback hardening

The architecture is intentionally strict: the runtime consumes only the internal config model, while Clash subscription support stays in adapter and provider layers. Current support is intentionally narrow: Clash ingestion covers nodes and groups, the runtime executes `DirectTcp` and `Trojan`, and more advanced Clash rule semantics are still outside scope.
