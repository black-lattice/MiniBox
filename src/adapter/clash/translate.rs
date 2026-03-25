use std::collections::BTreeSet;

use crate::config::external::{
    ExternalConfig, ExternalConfigSource, GroupInput, GroupStrategyInput, NodeInput, NodeKindInput,
    SubscriptionInput, TargetRefInput,
};
use crate::error::Error;

use super::model::{ClashProxy, ClashProxyGroup, ClashSubscription};

const GENERATED_SUBSCRIPTION_NAME: &str = "clash-subscription";

pub fn translate_document(
    source: &ExternalConfigSource,
    document: &ClashSubscription,
) -> Result<ExternalConfig, Error> {
    reject_unsupported_top_level_features(document)?;

    let subscription_name = GENERATED_SUBSCRIPTION_NAME.to_string();
    let nodes = translate_nodes(&document.proxies, &subscription_name)?;
    let node_names = collect_names("proxy", &nodes, |node| node.name.as_str())?;
    let group_names = collect_names("group", &document.proxy_groups, |group| group.name.trim())?;
    ensure_disjoint_names(&node_names, &group_names)?;
    let groups =
        translate_groups(&document.proxy_groups, &node_names, &group_names, &subscription_name)?;

    Ok(ExternalConfig {
        nodes,
        groups,
        subscriptions: vec![SubscriptionInput { name: subscription_name, source: source.clone() }],
        ..ExternalConfig::default()
    })
}

fn reject_unsupported_top_level_features(document: &ClashSubscription) -> Result<(), Error> {
    if !document.rules.is_empty() {
        return Err(Error::unsupported(
            "Clash rule-level semantics are not supported at level B: found top-level 'rules'",
        ));
    }

    if !document.rule_providers.is_empty() {
        return Err(Error::unsupported(
            "Clash rule-level semantics are not supported at level B: found top-level 'rule-providers'",
        ));
    }

    if document.script.is_some() {
        return Err(Error::unsupported(
            "Clash rule-level semantics are not supported at level B: found top-level 'script'",
        ));
    }

    if !document.proxy_providers.is_empty() {
        return Err(Error::unsupported(
            "Clash proxy-provider indirection is not supported at level B",
        ));
    }

    Ok(())
}

fn translate_nodes(
    proxies: &[ClashProxy],
    subscription_name: &str,
) -> Result<Vec<NodeInput>, Error> {
    let mut nodes = Vec::with_capacity(proxies.len());
    let mut seen = BTreeSet::new();

    for proxy in proxies {
        let name = normalize_name("proxy", proxy.name.as_str())?;
        if !seen.insert(name.clone()) {
            return Err(Error::validation(format!("duplicate proxy name '{}'", name)));
        }

        let kind = normalize_name("proxy type", proxy.kind.as_str())?;
        let lower_kind = kind.to_ascii_lowercase();
        if matches!(lower_kind.as_str(), "direct" | "reject" | "pass") {
            return Err(Error::unsupported(format!(
                "proxy '{}' uses Clash built-in type '{}', which does not map to an outbound node",
                name, kind
            )));
        }

        let server = proxy.server.as_deref().map(str::trim).filter(|value| !value.is_empty());
        let Some(server) = server else {
            return Err(Error::unsupported(format!(
                "proxy '{}' of type '{}' cannot be translated without both server and port",
                name, kind
            )));
        };
        let Some(port) = proxy.port else {
            return Err(Error::unsupported(format!(
                "proxy '{}' of type '{}' cannot be translated without both server and port",
                name, kind
            )));
        };

        match lower_kind.as_str() {
            "trojan" => {
                let password = proxy
                    .password
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        Error::unsupported(format!(
                            "proxy '{}' of type '{}' cannot be translated without password",
                            name, kind
                        ))
                    })?;
                nodes.push(NodeInput {
                    name,
                    kind: NodeKindInput::Trojan,
                    address: None,
                    server: Some(server.to_string()),
                    port: Some(port),
                    password: Some(password.to_string()),
                    sni: proxy.sni.clone(),
                    skip_cert_verify: proxy.skip_cert_verify,
                    provider: None,
                    subscription: Some(subscription_name.to_string()),
                });
            }
            _ => {
                return Err(Error::unimplemented(format!(
                    "Clash proxy '{}' uses type '{}', but MiniBox does not yet execute imported outbound proxy node protocols",
                    name, kind
                )));
            }
        }
    }

    Ok(nodes)
}

fn translate_groups(
    groups: &[ClashProxyGroup],
    node_names: &BTreeSet<String>,
    group_names: &BTreeSet<String>,
    subscription_name: &str,
) -> Result<Vec<GroupInput>, Error> {
    let mut translated = Vec::with_capacity(groups.len());

    for group in groups {
        let name = normalize_name("group", group.name.as_str())?;
        if !group.r#use.is_empty() {
            return Err(Error::unsupported(format!(
                "group '{}' uses provider-backed 'use' references, which are not supported at level B",
                name
            )));
        }

        let kind = normalize_name("group type", group.kind.as_str())?;
        let strategy = match kind.to_ascii_lowercase().as_str() {
            "select" => GroupStrategyInput::Select,
            "fallback" => GroupStrategyInput::Fallback,
            "url-test" => {
                return Err(Error::unsupported(format!(
                    "group '{}' uses strategy 'url-test', which is not supported at level B",
                    name
                )));
            }
            other => {
                return Err(Error::unsupported(format!(
                    "group '{}' uses strategy '{}', which is not supported at level B",
                    name, other
                )));
            }
        };

        if group.proxies.is_empty() {
            return Err(Error::validation(format!(
                "group '{}' must contain at least one member",
                name
            )));
        }

        let members = group
            .proxies
            .iter()
            .map(|member| translate_group_member(name.as_str(), member, node_names, group_names))
            .collect::<Result<Vec<_>, _>>()?;

        translated.push(GroupInput {
            name,
            strategy,
            members,
            provider: None,
            subscription: Some(subscription_name.to_string()),
        });
    }

    Ok(translated)
}

fn translate_group_member(
    group_name: &str,
    raw_member: &str,
    node_names: &BTreeSet<String>,
    group_names: &BTreeSet<String>,
) -> Result<TargetRefInput, Error> {
    let member = normalize_name("group member", raw_member)?;
    let matches_node = node_names.contains(member.as_str());
    let matches_group = group_names.contains(member.as_str());

    match (matches_node, matches_group) {
        (true, false) => Ok(TargetRefInput::Node(member)),
        (false, true) => Ok(TargetRefInput::Group(member)),
        (true, true) => Err(Error::validation(format!(
            "group '{}' references '{}', which is ambiguous because it matches both a proxy and a group",
            group_name, member
        ))),
        (false, false) => Err(Error::validation(format!(
            "group '{}' references missing proxy or group '{}'",
            group_name, member
        ))),
    }
}

fn collect_names<T>(
    kind: &str,
    items: &[T],
    mut selector: impl FnMut(&T) -> &str,
) -> Result<BTreeSet<String>, Error> {
    let mut names = BTreeSet::new();

    for item in items {
        let name = normalize_name(kind, selector(item))?;
        if !names.insert(name.clone()) {
            return Err(Error::validation(format!("duplicate {kind} name '{}'", name)));
        }
    }

    Ok(names)
}

fn ensure_disjoint_names(
    node_names: &BTreeSet<String>,
    group_names: &BTreeSet<String>,
) -> Result<(), Error> {
    for name in node_names {
        if group_names.contains(name.as_str()) {
            return Err(Error::validation(format!(
                "Clash document reuses name '{}' for both a proxy and a group",
                name
            )));
        }
    }

    Ok(())
}

fn normalize_name(kind: &str, raw: &str) -> Result<String, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::validation(format!("{kind} name must not be empty")));
    }

    Ok(trimmed.to_string())
}
