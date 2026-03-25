use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListenerProtocolInput {
    Socks5,
    HttpConnect,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupStrategyInput {
    Select,
    Fallback,
    UrlTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NodeKindInput {
    #[default]
    DirectTcp,
    Trojan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetRefInput {
    Node(String),
    Group(String),
}

impl TargetRefInput {
    pub fn node(name: impl Into<String>) -> Self {
        Self::Node(name.into())
    }

    pub fn group(name: impl Into<String>) -> Self {
        Self::Group(name.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalConfigSource {
    LocalFile { path: String },
    ClashSubscription { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerInput {
    pub name: String,
    pub bind: String,
    pub protocol: ListenerProtocolInput,
    pub target: TargetRefInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInput {
    pub name: String,
    #[serde(default)]
    pub kind: NodeKindInput,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub sni: Option<String>,
    #[serde(default)]
    pub skip_cert_verify: bool,
    pub provider: Option<String>,
    pub subscription: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupInput {
    pub name: String,
    pub strategy: GroupStrategyInput,
    pub members: Vec<TargetRefInput>,
    pub provider: Option<String>,
    pub subscription: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LimitsInput {
    pub max_connections: Option<usize>,
    pub relay_buffer_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AdminInput {
    pub enabled: bool,
    pub bind: Option<String>,
    pub access_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionInput {
    pub name: String,
    pub source: ExternalConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub subscription: String,
    pub update_interval_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ExternalConfig {
    pub listeners: Vec<ListenerInput>,
    pub nodes: Vec<NodeInput>,
    pub groups: Vec<GroupInput>,
    pub subscriptions: Vec<SubscriptionInput>,
    pub providers: Vec<ProviderInput>,
    pub limits: LimitsInput,
    pub admin: AdminInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDocument {
    pub source: ExternalConfigSource,
    pub raw: String,
}

impl ExternalDocument {
    pub fn new(source: ExternalConfigSource, raw: impl Into<String>) -> Self {
        Self { source, raw: raw.into() }
    }
}
