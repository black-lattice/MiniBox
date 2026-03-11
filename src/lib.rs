pub mod adapter;
pub mod bootstrap;
pub mod config;
pub mod error;
pub mod listener;
pub mod logging;
pub mod metrics;
pub mod protocol;
pub mod provider;
pub mod relay;
pub mod runtime;
pub mod session;
pub mod subscription;
pub mod upstream;

pub fn status_line() -> &'static str {
    "wanglin-proxy can bind listeners, accept direct SOCKS5 and HTTP CONNECT tunnels, and relay TCP traffic with bounded buffers."
}
