pub mod adapter;
pub mod bootstrap;
pub mod config;
pub mod error;
pub mod health;
pub mod listener;
pub mod logging;
pub mod metrics;
pub mod operations;
pub mod protocol;
pub mod provider;
pub mod relay;
pub mod runtime;
pub mod session;
pub mod subscription;
pub mod upstream;

pub fn status_line() -> &'static str {
    "MiniBox can bind listeners, accept direct SOCKS5 and HTTP CONNECT tunnels, and relay TCP traffic with bounded buffers."
}
