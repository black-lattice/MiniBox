mod model;
mod parse;
mod translate;

use crate::config::external::{ExternalConfig, ExternalDocument};
use crate::config::internal::ActiveConfig;
use crate::error::Error;

pub use model::{ClashProxy, ClashProxyGroup, ClashSubscription};

#[derive(Debug, Clone, Copy, Default)]
pub struct ClashLevelBAdapter;

impl ClashLevelBAdapter {
    pub fn supported_scope(&self) -> &'static str {
        "nodes + groups"
    }

    pub fn parse(&self, document: &ExternalDocument) -> Result<ClashSubscription, Error> {
        parse::parse_document(document)
    }

    pub fn translate_external(
        &self,
        document: &ExternalDocument,
    ) -> Result<ExternalConfig, Error> {
        let parsed = self.parse(document)?;
        translate::translate_document(&document.source, &parsed)
    }

    pub fn translate(&self, document: &ExternalDocument) -> Result<ActiveConfig, Error> {
        let external = self.translate_external(document)?;
        ActiveConfig::from_external(external)
    }
}

#[cfg(test)]
mod tests {
    use super::ClashLevelBAdapter;
    use crate::config::external::ExternalConfigSource;
    use crate::config::external::ExternalDocument;
    use crate::config::internal::{ConfigOrigin, GroupStrategy, TargetRef};
    use crate::error::Error;

    #[test]
    fn translates_supported_proxies_and_groups_into_active_config() {
        let adapter = ClashLevelBAdapter;
        let document = ExternalDocument::new(
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            },
            r#"
proxies:
  - name: edge-a
    type: ss
    server: 1.1.1.1
    port: 443
  - name: edge-b
    type: trojan
    server: edge.example.com
    port: 8443
proxy-groups:
  - name: auto
    type: fallback
    proxies:
      - edge-b
  - name: primary
    type: select
    proxies:
      - edge-a
      - auto
"#,
        );

        let active = adapter
            .translate(&document)
            .expect("supported Clash document should translate");

        assert_eq!(active.nodes().len(), 2);
        assert_eq!(active.groups().len(), 2);
        assert_eq!(active.nodes()[0].name, "edge-a");
        assert_eq!(active.nodes()[0].address, "1.1.1.1:443");
        assert_eq!(
            active.nodes()[0].origin,
            ConfigOrigin::Subscription {
                subscription: "clash-subscription".to_string(),
            }
        );
        assert_eq!(active.groups()[0].name, "auto");
        assert_eq!(active.groups()[0].strategy, GroupStrategy::Fallback);
        assert_eq!(
            active.groups()[1].members,
            vec![
                TargetRef::Node("edge-a".to_string()),
                TargetRef::Group("auto".to_string()),
            ]
        );
        assert_eq!(active.subscriptions().len(), 1);
    }

    #[test]
    fn rejects_rule_level_features() {
        let adapter = ClashLevelBAdapter;
        let document = ExternalDocument::new(
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            },
            r#"
proxies:
  - name: edge-a
    type: ss
    server: 1.1.1.1
    port: 443
rules:
  - MATCH,DIRECT
"#,
        );

        let error = adapter
            .translate(&document)
            .expect_err("rule-level semantics must fail closed");

        assert_eq!(
            error,
            Error::unsupported(
                "Clash rule-level semantics are not supported at level B: found top-level 'rules'",
            )
        );
    }
}
