pub mod external;
pub mod internal;
pub mod load;

pub use external::{
    AdminInput, ExternalConfig, ExternalConfigSource, ExternalDocument, GroupInput,
    GroupStrategyInput, LimitsInput, ListenerInput, ListenerProtocolInput, NodeInput,
    NodeKindInput, ProviderInput, SubscriptionInput, TargetRefInput,
};
pub use internal::{
    ActiveConfig, AdminConfig, ConfigOrigin, ConfigSnapshot, GroupConfig, GroupStrategy, Limits,
    ListenerConfig, NodeConfig, NodeKind, ProtocolKind, ProviderConfig, SubscriptionConfig,
    TargetRef, TrojanNodeConfig,
};

use crate::error::Error;

pub fn normalize(external: ExternalConfig) -> Result<ActiveConfig, Error> {
    ActiveConfig::from_external(external)
}
