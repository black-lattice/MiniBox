use crate::config::internal::ListenerConfig;

#[derive(Debug, Clone, Copy, Default)]
pub struct HttpConnectHandler;

impl HttpConnectHandler {
    pub fn listener_name<'a>(&self, listener: &'a ListenerConfig) -> &'a str {
        &listener.name
    }
}
