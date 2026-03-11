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

pub fn status_line() -> &'static str {
    "wanglin-proxy can bind listeners, accept downstream TCP sessions, and parse a narrow SOCKS5 CONNECT path."
}
