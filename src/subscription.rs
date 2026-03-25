use crate::adapter::clash::ClashLevelBAdapter;
use crate::config::external::ExternalConfigSource;
use crate::config::external::ExternalDocument;
use crate::config::internal::{
    ActiveConfig, ConfigOrigin, ConfigSnapshot, GroupConfig, GroupStrategy, TargetRef,
};
use crate::config::load::{load_local_document, read_local_file_document, read_source_document};
use crate::error::Error;
use crate::provider::cache::{CacheActivation, CacheActivationSource, CacheStore};

const STARTUP_TEMPLATE_PATH: &str = "config/example.yaml";
const STARTUP_ENTRY_GROUP_NAME: &str = "minibox-entry";

#[derive(Debug, Clone)]
pub struct SubscriptionPlan {
    pub source: ExternalConfigSource,
    pub cache_enabled: bool,
    pub rollback_enabled: bool,
}

impl Default for SubscriptionPlan {
    fn default() -> Self {
        Self {
            source: ExternalConfigSource::LocalFile { path: "config/example.yaml".to_string() },
            cache_enabled: true,
            rollback_enabled: true,
        }
    }
}

pub fn describe_update_flow() -> &'static [&'static str] {
    &[
        "read external input",
        "translate into internal config",
        "merge startup listener template when loading subscriptions",
        "validate internal snapshot",
        "persist last-known-good cache",
        "activate validated snapshot",
    ]
}

pub fn ingest_clash_document(
    adapter: &ClashLevelBAdapter,
    document: &ExternalDocument,
) -> Result<ActiveConfig, Error> {
    adapter.translate(document)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigActivationSource {
    LocalFile,
    FreshTranslation,
    LastKnownGoodCache,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigActivation {
    pub active_config: ActiveConfig,
    pub source: ConfigActivationSource,
    pub translation_error: Option<Error>,
}

pub async fn load_active_config_from_source(
    adapter: &ClashLevelBAdapter,
    cache: Option<&CacheStore>,
    source: &ExternalConfigSource,
) -> Result<ConfigActivation, Error> {
    let document = read_source_document(source).await?;
    load_active_config_from_document(adapter, cache, &document)
}

pub fn load_active_config_from_document(
    adapter: &ClashLevelBAdapter,
    cache: Option<&CacheStore>,
    document: &ExternalDocument,
) -> Result<ConfigActivation, Error> {
    match &document.source {
        ExternalConfigSource::LocalFile { .. } => Ok(ConfigActivation {
            active_config: load_local_document(document)?,
            source: ConfigActivationSource::LocalFile,
            translation_error: None,
        }),
        ExternalConfigSource::ClashSubscription { .. } => {
            match ingest_clash_document(adapter, document) {
                Ok(translated) => {
                    let template = load_startup_template()?;
                    let candidate = merge_startup_template(template, translated)?;

                    if let Some(cache) = cache {
                        map_cache_activation(cache.activate_candidate(Ok(candidate)))
                    } else {
                        Ok(ConfigActivation {
                            active_config: candidate,
                            source: ConfigActivationSource::FreshTranslation,
                            translation_error: None,
                        })
                    }
                }
                Err(error) => {
                    if let Some(cache) = cache {
                        map_cache_activation(cache.activate_candidate(Err(error)))
                    } else {
                        Err(error)
                    }
                }
            }
        }
    }
}

fn load_startup_template() -> Result<ActiveConfig, Error> {
    let document = read_local_file_document(STARTUP_TEMPLATE_PATH).map_err(|error| {
        Error::io(format!(
            "failed to load startup listener template '{}': {error}",
            STARTUP_TEMPLATE_PATH
        ))
    })?;

    load_local_document(&document)
}

fn merge_startup_template(
    template: ActiveConfig,
    subscription: ActiveConfig,
) -> Result<ActiveConfig, Error> {
    if subscription.nodes().is_empty() && subscription.groups().is_empty() {
        return merge_template_only(template, subscription);
    }

    let mut groups = Vec::with_capacity(subscription.groups().len() + 1);
    groups.push(bridge_entry_group(&subscription)?);
    groups.extend(subscription.groups().iter().cloned());

    let merged = ConfigSnapshot::from_parts(
        template.listeners().to_vec(),
        subscription.nodes().to_vec(),
        groups,
        subscription.subscriptions().to_vec(),
        subscription.providers().to_vec(),
        template.limits().clone(),
        template.admin().clone(),
    )?;

    ActiveConfig::new(merged)
}

fn merge_template_only(
    template: ActiveConfig,
    subscription: ActiveConfig,
) -> Result<ActiveConfig, Error> {
    let merged = ConfigSnapshot::from_parts(
        template.listeners().to_vec(),
        template.nodes().to_vec(),
        template.groups().to_vec(),
        subscription.subscriptions().to_vec(),
        subscription.providers().to_vec(),
        template.limits().clone(),
        template.admin().clone(),
    )?;

    ActiveConfig::new(merged)
}

fn bridge_entry_group(subscription: &ActiveConfig) -> Result<GroupConfig, Error> {
    let members = if !subscription.groups().is_empty() {
        subscription
            .groups()
            .iter()
            .map(|group| TargetRef::Group(group.name.clone()))
            .collect::<Vec<_>>()
    } else {
        subscription
            .nodes()
            .iter()
            .map(|node| TargetRef::Node(node.name.clone()))
            .collect::<Vec<_>>()
    };

    if members.is_empty() {
        return Err(Error::validation(
            "subscription source did not yield any nodes or groups to attach the local listener template",
        ));
    }

    Ok(GroupConfig {
        name: STARTUP_ENTRY_GROUP_NAME.to_string(),
        strategy: GroupStrategy::Select,
        members,
        origin: ConfigOrigin::Inline,
    })
}

fn map_cache_activation(
    activation: Result<CacheActivation, Error>,
) -> Result<ConfigActivation, Error> {
    activation.map(|activation| ConfigActivation {
        active_config: activation.active_config,
        source: match activation.source {
            CacheActivationSource::FreshTranslation => ConfigActivationSource::FreshTranslation,
            CacheActivationSource::LastKnownGoodCache => ConfigActivationSource::LastKnownGoodCache,
        },
        translation_error: activation.translation_error,
    })
}

#[cfg(test)]
mod tests {
    use super::{STARTUP_ENTRY_GROUP_NAME, merge_startup_template};
    use crate::config::external::{
        ExternalConfig, ExternalConfigSource, GroupInput, GroupStrategyInput, ListenerInput,
        ListenerProtocolInput, NodeInput, SubscriptionInput, TargetRefInput,
    };
    use crate::config::internal::{ActiveConfig, GroupStrategy, TargetRef};

    #[test]
    fn merge_startup_template_attaches_subscription_nodes_to_template_listener() {
        let template = ActiveConfig::from_external(ExternalConfig {
            listeners: vec![ListenerInput {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ListenerProtocolInput::Socks5,
                target: TargetRefInput::group(STARTUP_ENTRY_GROUP_NAME),
            }],
            nodes: vec![NodeInput {
                name: "default-upstream".to_string(),
                kind: crate::config::external::NodeKindInput::DirectTcp,
                address: None,
                server: None,
                port: None,
                password: None,
                sni: None,
                skip_cert_verify: false,
                provider: None,
                subscription: None,
            }],
            groups: vec![GroupInput {
                name: STARTUP_ENTRY_GROUP_NAME.to_string(),
                strategy: GroupStrategyInput::Select,
                members: vec![TargetRefInput::node("default-upstream")],
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        })
        .expect("template should normalize");

        let subscription = ActiveConfig::from_external(ExternalConfig {
            nodes: vec![NodeInput {
                name: "trojan-a".to_string(),
                kind: crate::config::external::NodeKindInput::Trojan,
                address: None,
                server: Some("127.0.0.1".to_string()),
                port: Some(443),
                password: Some("secret".to_string()),
                sni: Some("example.com".to_string()),
                skip_cert_verify: false,
                provider: None,
                subscription: Some("remote".to_string()),
            }],
            groups: vec![GroupInput {
                name: "proxy".to_string(),
                strategy: GroupStrategyInput::Select,
                members: vec![TargetRefInput::node("trojan-a")],
                provider: None,
                subscription: Some("remote".to_string()),
            }],
            subscriptions: vec![SubscriptionInput {
                name: "remote".to_string(),
                source: ExternalConfigSource::ClashSubscription {
                    url: "http://example.com/sub".to_string(),
                },
            }],
            ..ExternalConfig::default()
        })
        .expect("subscription should normalize");

        let merged = merge_startup_template(template, subscription).expect("merge should work");

        assert_eq!(merged.listeners().len(), 1);
        assert_eq!(merged.nodes().len(), 1);
        assert_eq!(merged.groups().len(), 2);
        assert_eq!(
            merged.listeners()[0].target,
            TargetRef::Group(STARTUP_ENTRY_GROUP_NAME.to_string())
        );
        assert_eq!(merged.groups()[0].name, STARTUP_ENTRY_GROUP_NAME);
        assert_eq!(merged.groups()[0].strategy, GroupStrategy::Select);
        assert_eq!(
            merged
                .resolve_target_node(&merged.listeners()[0].target)
                .expect("listener should resolve")
                .name,
            "trojan-a"
        );
    }

    #[test]
    fn merge_startup_template_keeps_local_template_for_empty_subscription() {
        let template = ActiveConfig::from_external(ExternalConfig {
            listeners: vec![ListenerInput {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ListenerProtocolInput::Socks5,
                target: TargetRefInput::group(STARTUP_ENTRY_GROUP_NAME),
            }],
            nodes: vec![NodeInput {
                name: "default-upstream".to_string(),
                kind: crate::config::external::NodeKindInput::DirectTcp,
                address: None,
                server: None,
                port: None,
                password: None,
                sni: None,
                skip_cert_verify: false,
                provider: None,
                subscription: None,
            }],
            groups: vec![GroupInput {
                name: STARTUP_ENTRY_GROUP_NAME.to_string(),
                strategy: GroupStrategyInput::Select,
                members: vec![TargetRefInput::node("default-upstream")],
                provider: None,
                subscription: None,
            }],
            ..ExternalConfig::default()
        })
        .expect("template should normalize");

        let merged = merge_startup_template(template.clone(), ActiveConfig::default())
            .expect("empty subscription should keep the template");

        assert_eq!(merged, template);
    }
}
