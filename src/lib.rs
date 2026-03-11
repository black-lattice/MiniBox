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
pub mod subscription;

pub fn status_line() -> &'static str {
    "wanglin-proxy has config-driven listener scaffolding and a narrow SOCKS5 protocol foundation."
}
