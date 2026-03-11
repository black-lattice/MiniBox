use std::fmt::{Display, Formatter};
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialTargetHost {
    Ip(IpAddr),
    Domain(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialTarget {
    pub host: DialTargetHost,
    pub port: u16,
}

impl Display for DialTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.host {
            DialTargetHost::Ip(address) => write!(f, "{address}:{}", self.port),
            DialTargetHost::Domain(domain) => write!(f, "{domain}:{}", self.port),
        }
    }
}
