use crate::config::internal::ListenerConfig;

#[derive(Debug, Clone, Copy, Default)]
pub struct Socks5Handler;

impl Socks5Handler {
    pub fn listener_name<'a>(&self, listener: &'a ListenerConfig) -> &'a str {
        &listener.name
    }
}
