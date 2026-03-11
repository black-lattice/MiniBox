use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct ClashSubscription {
    pub proxies: Vec<ClashProxy>,
    pub proxy_groups: Vec<ClashProxyGroup>,
    pub rules: Vec<String>,
    pub rule_providers: BTreeMap<String, String>,
    pub proxy_providers: BTreeMap<String, String>,
    pub script: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ClashProxy {
    pub name: String,
    pub kind: String,
    pub server: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default)]
pub struct ClashProxyGroup {
    pub name: String,
    pub kind: String,
    pub proxies: Vec<String>,
    pub r#use: Vec<String>,
}
