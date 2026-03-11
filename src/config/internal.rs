use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::config::external::{
    AdminInput, ExternalConfig, GroupStrategyInput, ListenerProtocolInput, TargetRefInput,
};
use crate::error::Error;

pub const DEFAULT_MAX_CONNECTIONS: usize = 1024;
pub const DEFAULT_RELAY_BUFFER_BYTES: usize = 16 * 1024;
pub const MIN_RELAY_BUFFER_BYTES: usize = 1024;
pub const MAX_RELAY_BUFFER_BYTES: usize = 1024 * 1024;
pub const MAX_SAFE_CONNECTIONS: usize = 65_536;
pub const DEFAULT_ADMIN_BIND: &str = "127.0.0.1:9090";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolKind {
    Socks5,
    HttpConnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupStrategy {
    Select,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetRef {
    Node(String),
    Group(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigOrigin {
    Inline,
    Provider {
        provider: String,
        subscription: String,
    },
    Subscription {
        subscription: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerConfig {
    pub name: String,
    pub bind: String,
    pub protocol: ProtocolKind,
    pub target: TargetRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeConfig {
    pub name: String,
    pub address: String,
    pub origin: ConfigOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupConfig {
    pub name: String,
    pub strategy: GroupStrategy,
    pub members: Vec<TargetRef>,
    pub origin: ConfigOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionConfig {
    pub name: String,
    pub source: crate::config::external::ExternalConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    pub name: String,
    pub subscription: String,
    pub update_interval_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminConfig {
    pub enabled: bool,
    pub bind: Option<String>,
    pub access_token: Option<String>,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: None,
            access_token: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub max_connections: usize,
    pub relay_buffer_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            relay_buffer_bytes: DEFAULT_RELAY_BUFFER_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigSnapshot {
    listeners: Vec<ListenerConfig>,
    nodes: Vec<NodeConfig>,
    groups: Vec<GroupConfig>,
    subscriptions: Vec<SubscriptionConfig>,
    providers: Vec<ProviderConfig>,
    limits: Limits,
    admin: AdminConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveConfig {
    snapshot: Arc<ConfigSnapshot>,
}

impl ActiveConfig {
    pub fn new(snapshot: ConfigSnapshot) -> Result<Self, Error> {
        snapshot.validate()?;
        Ok(Self {
            snapshot: Arc::new(snapshot),
        })
    }

    pub fn from_external(external: ExternalConfig) -> Result<Self, Error> {
        Self::new(ConfigSnapshot::from_external(external)?)
    }

    pub fn snapshot(&self) -> &ConfigSnapshot {
        self.snapshot.as_ref()
    }

    pub fn listeners(&self) -> &[ListenerConfig] {
        &self.snapshot.listeners
    }

    pub fn nodes(&self) -> &[NodeConfig] {
        &self.snapshot.nodes
    }

    pub fn groups(&self) -> &[GroupConfig] {
        &self.snapshot.groups
    }

    pub fn subscriptions(&self) -> &[SubscriptionConfig] {
        &self.snapshot.subscriptions
    }

    pub fn providers(&self) -> &[ProviderConfig] {
        &self.snapshot.providers
    }

    pub fn limits(&self) -> &Limits {
        &self.snapshot.limits
    }

    pub fn admin(&self) -> &AdminConfig {
        &self.snapshot.admin
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.snapshot.validate()
    }
}

impl Default for ActiveConfig {
    fn default() -> Self {
        Self {
            snapshot: Arc::new(ConfigSnapshot::default()),
        }
    }
}

impl ConfigSnapshot {
    pub fn from_external(external: ExternalConfig) -> Result<Self, Error> {
        let subscriptions = normalize_subscriptions(external.subscriptions)?;
        let subscription_names =
            collect_owned_names(&subscriptions, |subscription| subscription.name.as_str());
        let providers = normalize_providers(external.providers, &subscription_names)?;
        let provider_subscriptions = provider_subscription_map(&providers)?;
        let nodes = normalize_nodes(external.nodes, &provider_subscriptions, &subscription_names)?;
        let groups = normalize_groups(
            external.groups,
            &provider_subscriptions,
            &subscription_names,
        )?;
        let listeners = normalize_listeners(external.listeners)?;
        let limits = normalize_limits(external.limits)?;
        let admin = normalize_admin(external.admin)?;

        let snapshot = Self {
            listeners,
            nodes,
            groups,
            subscriptions,
            providers,
            limits,
            admin,
        };

        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn listeners(&self) -> &[ListenerConfig] {
        &self.listeners
    }

    pub fn nodes(&self) -> &[NodeConfig] {
        &self.nodes
    }

    pub fn groups(&self) -> &[GroupConfig] {
        &self.groups
    }

    pub fn subscriptions(&self) -> &[SubscriptionConfig] {
        &self.subscriptions
    }

    pub fn providers(&self) -> &[ProviderConfig] {
        &self.providers
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn admin(&self) -> &AdminConfig {
        &self.admin
    }

    pub fn validate(&self) -> Result<(), Error> {
        ensure_unique_names("listener", &self.listeners, |listener| {
            listener.name.as_str()
        })?;
        ensure_unique_names("node", &self.nodes, |node| node.name.as_str())?;
        ensure_unique_names("group", &self.groups, |group| group.name.as_str())?;
        ensure_unique_names("subscription", &self.subscriptions, |subscription| {
            subscription.name.as_str()
        })?;
        ensure_unique_names("provider", &self.providers, |provider| {
            provider.name.as_str()
        })?;
        validate_limits(&self.limits)?;

        let node_names = collect_names(&self.nodes, |node| node.name.as_str());
        let group_names = collect_names(&self.groups, |group| group.name.as_str());
        let subscription_names = collect_names(&self.subscriptions, |subscription| {
            subscription.name.as_str()
        });
        let providers_by_name = named_index("provider", &self.providers, |provider| {
            provider.name.as_str()
        })?;

        for provider in &self.providers {
            if !subscription_names.contains(provider.subscription.as_str()) {
                return Err(Error::validation(format!(
                    "provider '{}' references missing subscription '{}'",
                    provider.name, provider.subscription
                )));
            }

            if provider.update_interval_secs == Some(0) {
                return Err(Error::validation(format!(
                    "provider '{}' has update_interval_secs=0, which is invalid",
                    provider.name
                )));
            }
        }

        for node in &self.nodes {
            validate_origin(
                "node",
                node.name.as_str(),
                &node.origin,
                &providers_by_name,
                &subscription_names,
            )?;

            if node.address.trim().is_empty() {
                return Err(Error::validation(format!(
                    "node '{}' must have a non-empty address",
                    node.name
                )));
            }
        }

        for group in &self.groups {
            validate_origin(
                "group",
                group.name.as_str(),
                &group.origin,
                &providers_by_name,
                &subscription_names,
            )?;

            if group.members.is_empty() {
                return Err(Error::validation(format!(
                    "group '{}' must contain at least one member",
                    group.name
                )));
            }

            for member in &group.members {
                validate_target_ref(
                    member,
                    &node_names,
                    &group_names,
                    format!("group '{}'", group.name).as_str(),
                )?;
            }
        }

        for listener in &self.listeners {
            if listener.bind.trim().is_empty() {
                return Err(Error::validation(format!(
                    "listener '{}' must have a non-empty bind address",
                    listener.name
                )));
            }

            validate_target_ref(
                &listener.target,
                &node_names,
                &group_names,
                format!("listener '{}'", listener.name).as_str(),
            )?;
        }

        Ok(())
    }
}

fn normalize_subscriptions(
    subscriptions: Vec<crate::config::external::SubscriptionInput>,
) -> Result<Vec<SubscriptionConfig>, Error> {
    let mut normalized = Vec::with_capacity(subscriptions.len());
    let mut seen = BTreeSet::new();

    for subscription in subscriptions {
        let name = normalize_name("subscription", subscription.name)?;
        if !seen.insert(name.clone()) {
            return Err(Error::validation(format!(
                "duplicate subscription name '{}'",
                name
            )));
        }

        normalized.push(SubscriptionConfig {
            name,
            source: subscription.source,
        });
    }

    Ok(normalized)
}

fn normalize_providers(
    providers: Vec<crate::config::external::ProviderInput>,
    subscription_names: &BTreeSet<String>,
) -> Result<Vec<ProviderConfig>, Error> {
    let mut normalized = Vec::with_capacity(providers.len());
    let mut seen = BTreeSet::new();

    for provider in providers {
        let name = normalize_name("provider", provider.name)?;
        if !seen.insert(name.clone()) {
            return Err(Error::validation(format!(
                "duplicate provider name '{}'",
                name
            )));
        }

        let subscription = normalize_ref_name("subscription", provider.subscription)?;
        if !subscription_names.contains(&subscription) {
            return Err(Error::validation(format!(
                "provider '{}' references missing subscription '{}'",
                name, subscription
            )));
        }

        if provider.update_interval_secs == Some(0) {
            return Err(Error::validation(format!(
                "provider '{}' has update_interval_secs=0, which is invalid",
                name
            )));
        }

        normalized.push(ProviderConfig {
            name,
            subscription,
            update_interval_secs: provider.update_interval_secs,
        });
    }

    Ok(normalized)
}

fn normalize_nodes(
    nodes: Vec<crate::config::external::NodeInput>,
    provider_subscriptions: &BTreeMap<String, String>,
    subscription_names: &BTreeSet<String>,
) -> Result<Vec<NodeConfig>, Error> {
    let mut normalized = Vec::with_capacity(nodes.len());
    let mut seen = BTreeSet::new();

    for node in nodes {
        let name = normalize_name("node", node.name)?;
        if !seen.insert(name.clone()) {
            return Err(Error::validation(format!("duplicate node name '{}'", name)));
        }

        let address = normalize_required_field("node", "address", node.address, &name)?;
        let origin = normalize_origin(
            "node",
            &name,
            node.provider,
            node.subscription,
            provider_subscriptions,
            subscription_names,
        )?;

        normalized.push(NodeConfig {
            name,
            address,
            origin,
        });
    }

    Ok(normalized)
}

fn normalize_groups(
    groups: Vec<crate::config::external::GroupInput>,
    provider_subscriptions: &BTreeMap<String, String>,
    subscription_names: &BTreeSet<String>,
) -> Result<Vec<GroupConfig>, Error> {
    let mut normalized = Vec::with_capacity(groups.len());
    let mut seen = BTreeSet::new();

    for group in groups {
        let name = normalize_name("group", group.name)?;
        if !seen.insert(name.clone()) {
            return Err(Error::validation(format!(
                "duplicate group name '{}'",
                name
            )));
        }

        if group.members.is_empty() {
            return Err(Error::validation(format!(
                "group '{}' must contain at least one member",
                name
            )));
        }

        let strategy = match group.strategy {
            GroupStrategyInput::Select => GroupStrategy::Select,
            GroupStrategyInput::Fallback => GroupStrategy::Fallback,
            GroupStrategyInput::UrlTest => {
                return Err(Error::unsupported(format!(
                    "group '{}' uses url-test strategy, which is not supported in the config foundation stage",
                    name
                )));
            }
        };

        let origin = normalize_origin(
            "group",
            &name,
            group.provider,
            group.subscription,
            provider_subscriptions,
            subscription_names,
        )?;
        let members = group
            .members
            .into_iter()
            .map(normalize_target_ref)
            .collect::<Result<Vec<_>, _>>()?;

        normalized.push(GroupConfig {
            name,
            strategy,
            members,
            origin,
        });
    }

    Ok(normalized)
}

fn normalize_listeners(
    listeners: Vec<crate::config::external::ListenerInput>,
) -> Result<Vec<ListenerConfig>, Error> {
    let mut normalized = Vec::with_capacity(listeners.len());
    let mut seen = BTreeSet::new();

    for listener in listeners {
        let name = normalize_name("listener", listener.name)?;
        if !seen.insert(name.clone()) {
            return Err(Error::validation(format!(
                "duplicate listener name '{}'",
                name
            )));
        }

        let bind = normalize_required_field("listener", "bind", listener.bind, &name)?;
        let protocol = match listener.protocol {
            ListenerProtocolInput::Socks5 => ProtocolKind::Socks5,
            ListenerProtocolInput::HttpConnect => ProtocolKind::HttpConnect,
            ListenerProtocolInput::Mixed => {
                return Err(Error::unsupported(format!(
                    "listener '{}' uses mixed protocol mode, which is not supported in the config foundation stage",
                    name
                )));
            }
        };

        normalized.push(ListenerConfig {
            name,
            bind,
            protocol,
            target: normalize_target_ref(listener.target)?,
        });
    }

    Ok(normalized)
}

fn normalize_limits(input: crate::config::external::LimitsInput) -> Result<Limits, Error> {
    let limits = Limits {
        max_connections: input.max_connections.unwrap_or(DEFAULT_MAX_CONNECTIONS),
        relay_buffer_bytes: input
            .relay_buffer_bytes
            .unwrap_or(DEFAULT_RELAY_BUFFER_BYTES),
    };

    validate_limits(&limits)?;
    Ok(limits)
}

fn normalize_admin(input: AdminInput) -> Result<AdminConfig, Error> {
    let bind = match (input.enabled, input.bind) {
        (true, Some(bind)) => Some(normalize_inline_field("admin", "bind", bind)?),
        (true, None) => Some(DEFAULT_ADMIN_BIND.to_string()),
        (false, Some(bind)) => Some(normalize_inline_field("admin", "bind", bind)?),
        (false, None) => None,
    };
    let access_token = match input.access_token {
        Some(token) => Some(normalize_inline_field("admin", "access_token", token)?),
        None => None,
    };

    Ok(AdminConfig {
        enabled: input.enabled,
        bind,
        access_token,
    })
}

fn normalize_origin(
    kind: &str,
    name: &str,
    provider: Option<String>,
    subscription: Option<String>,
    provider_subscriptions: &BTreeMap<String, String>,
    subscription_names: &BTreeSet<String>,
) -> Result<ConfigOrigin, Error> {
    match (provider, subscription) {
        (Some(provider), Some(subscription)) => Err(Error::unsupported(format!(
            "{kind} '{}' cannot reference both provider '{}' and subscription '{}' at the same time",
            name,
            provider.trim(),
            subscription.trim()
        ))),
        (Some(provider), None) => {
            let provider = normalize_ref_name("provider", provider)?;
            let Some(subscription) = provider_subscriptions.get(&provider) else {
                return Err(Error::validation(format!(
                    "{kind} '{}' references missing provider '{}'",
                    name, provider
                )));
            };
            Ok(ConfigOrigin::Provider {
                provider,
                subscription: subscription.clone(),
            })
        }
        (None, Some(subscription)) => {
            let subscription = normalize_ref_name("subscription", subscription)?;
            if !subscription_names.contains(&subscription) {
                return Err(Error::validation(format!(
                    "{kind} '{}' references missing subscription '{}'",
                    name, subscription
                )));
            }

            Ok(ConfigOrigin::Subscription { subscription })
        }
        (None, None) => Ok(ConfigOrigin::Inline),
    }
}

fn normalize_target_ref(target: TargetRefInput) -> Result<TargetRef, Error> {
    match target {
        TargetRefInput::Node(name) => Ok(TargetRef::Node(normalize_ref_name("node", name)?)),
        TargetRefInput::Group(name) => Ok(TargetRef::Group(normalize_ref_name("group", name)?)),
    }
}

fn validate_origin(
    kind: &str,
    name: &str,
    origin: &ConfigOrigin,
    providers_by_name: &BTreeMap<String, &ProviderConfig>,
    subscription_names: &BTreeSet<&str>,
) -> Result<(), Error> {
    match origin {
        ConfigOrigin::Inline => Ok(()),
        ConfigOrigin::Subscription { subscription } => {
            if subscription_names.contains(subscription.as_str()) {
                Ok(())
            } else {
                Err(Error::validation(format!(
                    "{kind} '{}' references missing subscription '{}'",
                    name, subscription
                )))
            }
        }
        ConfigOrigin::Provider {
            provider,
            subscription,
        } => {
            let Some(provider_config) = providers_by_name.get(provider.as_str()) else {
                return Err(Error::validation(format!(
                    "{kind} '{}' references missing provider '{}'",
                    name, provider
                )));
            };

            if provider_config.subscription != *subscription {
                return Err(Error::validation(format!(
                    "{kind} '{}' resolved provider '{}' to subscription '{}', but snapshot stored '{}'",
                    name, provider, provider_config.subscription, subscription
                )));
            }

            Ok(())
        }
    }
}

fn validate_target_ref(
    target: &TargetRef,
    node_names: &BTreeSet<&str>,
    group_names: &BTreeSet<&str>,
    owner: &str,
) -> Result<(), Error> {
    match target {
        TargetRef::Node(name) => {
            if node_names.contains(name.as_str()) {
                Ok(())
            } else {
                Err(Error::validation(format!(
                    "{owner} references missing node '{}'",
                    name
                )))
            }
        }
        TargetRef::Group(name) => {
            if group_names.contains(name.as_str()) {
                Ok(())
            } else {
                Err(Error::validation(format!(
                    "{owner} references missing group '{}'",
                    name
                )))
            }
        }
    }
}

fn validate_limits(limits: &Limits) -> Result<(), Error> {
    if limits.max_connections == 0 {
        return Err(Error::validation(
            "unsafe limits: max_connections must be greater than zero",
        ));
    }

    if limits.max_connections > MAX_SAFE_CONNECTIONS {
        return Err(Error::validation(format!(
            "unsafe limits: max_connections={} exceeds the current safety cap of {}",
            limits.max_connections, MAX_SAFE_CONNECTIONS
        )));
    }

    if limits.relay_buffer_bytes < MIN_RELAY_BUFFER_BYTES {
        return Err(Error::validation(format!(
            "unsafe limits: relay_buffer_bytes={} is below the minimum {}",
            limits.relay_buffer_bytes, MIN_RELAY_BUFFER_BYTES
        )));
    }

    if limits.relay_buffer_bytes > MAX_RELAY_BUFFER_BYTES {
        return Err(Error::validation(format!(
            "unsafe limits: relay_buffer_bytes={} exceeds the maximum {}",
            limits.relay_buffer_bytes, MAX_RELAY_BUFFER_BYTES
        )));
    }

    Ok(())
}

fn normalize_name(kind: &str, raw: String) -> Result<String, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::validation(format!("{kind} name must not be empty")));
    }

    Ok(trimmed.to_string())
}

fn normalize_ref_name(kind: &str, raw: String) -> Result<String, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::validation(format!(
            "{kind} reference must not be empty"
        )));
    }

    Ok(trimmed.to_string())
}

fn normalize_required_field(
    kind: &str,
    field: &str,
    raw: String,
    name: &str,
) -> Result<String, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::validation(format!(
            "{kind} '{}' must have a non-empty {field}",
            name
        )));
    }

    Ok(trimmed.to_string())
}

fn normalize_inline_field(kind: &str, field: &str, raw: String) -> Result<String, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::validation(format!(
            "{kind} {field} must not be empty when set"
        )));
    }

    Ok(trimmed.to_string())
}

fn ensure_unique_names<T, F>(kind: &str, items: &[T], name_of: F) -> Result<(), Error>
where
    F: Fn(&T) -> &str,
{
    let mut seen = BTreeSet::new();
    for item in items {
        let name = name_of(item);
        if !seen.insert(name) {
            return Err(Error::validation(format!(
                "duplicate {kind} name '{}'",
                name
            )));
        }
    }

    Ok(())
}

fn collect_names<'a, T, F>(items: &'a [T], name_of: F) -> BTreeSet<&'a str>
where
    F: Fn(&'a T) -> &'a str,
{
    items.iter().map(name_of).collect()
}

fn collect_owned_names<T, F>(items: &[T], name_of: F) -> BTreeSet<String>
where
    F: Fn(&T) -> &str,
{
    items.iter().map(|item| name_of(item).to_string()).collect()
}

fn named_index<'a, T, F>(
    kind: &str,
    items: &'a [T],
    name_of: F,
) -> Result<BTreeMap<String, &'a T>, Error>
where
    F: Fn(&'a T) -> &'a str,
{
    let mut index = BTreeMap::new();
    for item in items {
        let name = name_of(item);
        if index.insert(name.to_string(), item).is_some() {
            return Err(Error::validation(format!(
                "duplicate {kind} name '{}'",
                name
            )));
        }
    }

    Ok(index)
}

fn provider_subscription_map(
    providers: &[ProviderConfig],
) -> Result<BTreeMap<String, String>, Error> {
    let mut provider_subscriptions = BTreeMap::new();
    for provider in providers {
        if provider_subscriptions
            .insert(provider.name.clone(), provider.subscription.clone())
            .is_some()
        {
            return Err(Error::validation(format!(
                "duplicate provider name '{}'",
                provider.name
            )));
        }
    }

    Ok(provider_subscriptions)
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveConfig, ConfigOrigin, DEFAULT_ADMIN_BIND, DEFAULT_MAX_CONNECTIONS,
        DEFAULT_RELAY_BUFFER_BYTES,
    };
    use crate::config::external::{
        AdminInput, ExternalConfig, ExternalConfigSource, GroupInput, GroupStrategyInput,
        LimitsInput, ListenerInput, ListenerProtocolInput, NodeInput, ProviderInput,
        SubscriptionInput, TargetRefInput,
    };
    use crate::config::internal::{GroupStrategy, ProtocolKind, TargetRef};
    use crate::error::Error;

    #[test]
    fn normalizes_external_config_into_active_snapshot() {
        let config = ExternalConfig {
            listeners: vec![ListenerInput {
                name: " local-socks ".to_string(),
                bind: " 127.0.0.1:1080 ".to_string(),
                protocol: ListenerProtocolInput::Socks5,
                target: TargetRefInput::group("primary"),
            }],
            nodes: vec![
                NodeInput {
                    name: " local-a ".to_string(),
                    address: " 1.1.1.1:443 ".to_string(),
                    provider: None,
                    subscription: None,
                },
                NodeInput {
                    name: " remote-a ".to_string(),
                    address: " 2.2.2.2:443 ".to_string(),
                    provider: Some(" provider-main ".to_string()),
                    subscription: None,
                },
            ],
            groups: vec![GroupInput {
                name: " primary ".to_string(),
                strategy: GroupStrategyInput::Fallback,
                members: vec![
                    TargetRefInput::node(" local-a "),
                    TargetRefInput::node(" remote-a "),
                ],
                provider: None,
                subscription: Some(" remote-sub ".to_string()),
            }],
            subscriptions: vec![SubscriptionInput {
                name: " remote-sub ".to_string(),
                source: ExternalConfigSource::ClashSubscription {
                    url: "https://example.com/sub".to_string(),
                },
            }],
            providers: vec![ProviderInput {
                name: " provider-main ".to_string(),
                subscription: " remote-sub ".to_string(),
                update_interval_secs: Some(3600),
            }],
            limits: LimitsInput::default(),
            admin: AdminInput {
                enabled: true,
                bind: None,
                access_token: Some(" secret ".to_string()),
            },
        };

        let active = ActiveConfig::from_external(config).expect("config should normalize");

        assert_eq!(active.listeners().len(), 1);
        assert_eq!(active.nodes().len(), 2);
        assert_eq!(active.groups().len(), 1);
        assert_eq!(active.subscriptions().len(), 1);
        assert_eq!(active.providers().len(), 1);
        assert_eq!(active.limits().max_connections, DEFAULT_MAX_CONNECTIONS);
        assert_eq!(
            active.limits().relay_buffer_bytes,
            DEFAULT_RELAY_BUFFER_BYTES
        );
        assert_eq!(active.admin().bind.as_deref(), Some(DEFAULT_ADMIN_BIND));
        assert_eq!(active.admin().access_token.as_deref(), Some("secret"));
        assert_eq!(active.listeners()[0].name, "local-socks");
        assert_eq!(active.listeners()[0].bind, "127.0.0.1:1080");
        assert_eq!(active.listeners()[0].protocol, ProtocolKind::Socks5);
        assert_eq!(
            active.listeners()[0].target,
            TargetRef::Group("primary".to_string())
        );
        assert_eq!(active.groups()[0].strategy, GroupStrategy::Fallback);
        assert_eq!(
            active.groups()[0].origin,
            ConfigOrigin::Subscription {
                subscription: "remote-sub".to_string()
            }
        );
    }

    #[test]
    fn rejects_duplicate_names() {
        let config = ExternalConfig {
            nodes: vec![
                NodeInput {
                    name: "dup".to_string(),
                    address: "1.1.1.1:443".to_string(),
                    provider: None,
                    subscription: None,
                },
                NodeInput {
                    name: " dup ".to_string(),
                    address: "2.2.2.2:443".to_string(),
                    provider: None,
                    subscription: None,
                },
            ],
            ..ExternalConfig::default()
        };

        let error = ActiveConfig::from_external(config).expect_err("duplicate name must fail");
        assert_eq!(error, Error::validation("duplicate node name 'dup'"));
    }

    #[test]
    fn rejects_missing_references() {
        let config = ExternalConfig {
            listeners: vec![ListenerInput {
                name: "socks".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ListenerProtocolInput::Socks5,
                target: TargetRefInput::group("missing"),
            }],
            ..ExternalConfig::default()
        };

        let error = ActiveConfig::from_external(config).expect_err("missing reference must fail");
        assert_eq!(
            error,
            Error::validation("listener 'socks' references missing group 'missing'")
        );
    }

    #[test]
    fn rejects_unsupported_origin_combination() {
        let config = ExternalConfig {
            subscriptions: vec![SubscriptionInput {
                name: "remote".to_string(),
                source: ExternalConfigSource::ClashSubscription {
                    url: "https://example.com/sub".to_string(),
                },
            }],
            providers: vec![ProviderInput {
                name: "provider-a".to_string(),
                subscription: "remote".to_string(),
                update_interval_secs: None,
            }],
            nodes: vec![NodeInput {
                name: "node-a".to_string(),
                address: "1.1.1.1:443".to_string(),
                provider: Some("provider-a".to_string()),
                subscription: Some("remote".to_string()),
            }],
            ..ExternalConfig::default()
        };

        let error =
            ActiveConfig::from_external(config).expect_err("unsupported origin combination");
        assert_eq!(
            error,
            Error::unsupported(
                "node 'node-a' cannot reference both provider 'provider-a' and subscription 'remote' at the same time",
            )
        );
    }

    #[test]
    fn rejects_empty_groups() {
        let config = ExternalConfig {
            groups: vec![GroupInput {
                name: "empty".to_string(),
                strategy: GroupStrategyInput::Select,
                members: Vec::new(),
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        };

        let error = ActiveConfig::from_external(config).expect_err("empty group must fail");
        assert_eq!(
            error,
            Error::validation("group 'empty' must contain at least one member")
        );
    }

    #[test]
    fn rejects_unsafe_limits() {
        let config = ExternalConfig {
            limits: LimitsInput {
                max_connections: Some(0),
                relay_buffer_bytes: Some(512),
            },
            ..ExternalConfig::default()
        };

        let error = ActiveConfig::from_external(config).expect_err("unsafe limits must fail");
        assert_eq!(
            error,
            Error::validation("unsafe limits: max_connections must be greater than zero")
        );
    }

    #[test]
    fn rejects_unsupported_listener_protocol() {
        let config = ExternalConfig {
            listeners: vec![ListenerInput {
                name: "mixed".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ListenerProtocolInput::Mixed,
                target: TargetRefInput::node("node-a"),
            }],
            nodes: vec![NodeInput {
                name: "node-a".to_string(),
                address: "1.1.1.1:443".to_string(),
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        };

        let error =
            ActiveConfig::from_external(config).expect_err("mixed listeners are unsupported");
        assert_eq!(
            error,
            Error::unsupported(
                "listener 'mixed' uses mixed protocol mode, which is not supported in the config foundation stage",
            )
        );
    }
}
