# MiniBox Release Checklist

Use this before tagging or shipping a build.

## Required Checks

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

## Functional Checks

- Start with `cargo run`
- Start with a Clash subscription URL
- Confirm startup prints the active source and cache status
- Confirm `127.0.0.1:1080` accepts SOCKS5
- Confirm `127.0.0.1:8080` accepts HTTP CONNECT
- Confirm a Trojan subscription can reach an external target

## Support Boundary

- Clash rules are not fully supported
- TUN and transparent proxying are not supported
- Windows support is not included in the current MVP

## Failure Handling

- Verify subscription translation failure falls back to the cache
- Verify the startup summary shows the selected listener target
- Verify the logs explain whether fresh translation or cache activation was used
