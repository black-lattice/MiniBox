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
        "subscription parsing and boundary validation; executable imported outbound nodes are still pending"
    }

    pub fn parse(&self, document: &ExternalDocument) -> Result<ClashSubscription, Error> {
        parse::parse_document(document)
    }

    pub fn translate_external(&self, document: &ExternalDocument) -> Result<ExternalConfig, Error> {
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
    use crate::config::internal::NodeKind;
    use crate::error::Error;

    #[test]
    fn translates_empty_subscription_into_metadata_only() {
        let adapter = ClashLevelBAdapter;
        let document = ExternalDocument::new(
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            },
            "# translation scaffolding only in this stage\n",
        );

        let active = adapter
            .translate(&document)
            .expect("empty Clash document should still produce subscription metadata");

        assert!(active.nodes().is_empty());
        assert!(active.groups().is_empty());
        assert_eq!(active.subscriptions().len(), 1);
    }

    #[test]
    fn rejects_proxy_nodes_until_runtime_supports_outbound_protocols() {
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
"#,
        );

        let error = adapter.translate(&document).expect_err(
            "runtime should reject imported proxy nodes until outbound execution exists",
        );

        assert_eq!(
            error,
            Error::unimplemented(
                "Clash proxy 'edge-a' uses type 'ss', but MiniBox does not yet execute imported outbound proxy node protocols",
            )
        );
    }

    #[test]
    fn translates_trojan_proxy_nodes_into_internal_config() {
        let adapter = ClashLevelBAdapter;
        let document = ExternalDocument::new(
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            },
            r#"
proxies:
  - name: edge-a
    type: trojan
    server: edge.example.com
    port: 443
    password: secret
    sni: edge.example.com
    skip-cert-verify: true
"#,
        );

        let active = adapter
            .translate(&document)
            .expect("Trojan proxy should translate into internal config");

        assert_eq!(active.nodes().len(), 1);
        assert_eq!(active.nodes()[0].kind, NodeKind::Trojan);
        let trojan = active.nodes()[0].trojan.as_ref().expect("Trojan settings should be present");
        assert_eq!(trojan.server, "edge.example.com");
        assert_eq!(trojan.port, 443);
        assert_eq!(trojan.password, "secret");
        assert_eq!(trojan.sni.as_deref(), Some("edge.example.com"));
        assert!(trojan.skip_cert_verify);
    }

    #[test]
    fn rejects_rule_level_features() {
        let adapter = ClashLevelBAdapter;
        let document = ExternalDocument::new(
            ExternalConfigSource::ClashSubscription {
                url: "https://example.com/subscription".to_string(),
            },
            r#"
rules:
  - MATCH,DIRECT
"#,
        );

        let error =
            adapter.translate(&document).expect_err("rule-level semantics must fail closed");

        assert_eq!(
            error,
            Error::unsupported(
                "Clash rule-level semantics are not supported at level B: found top-level 'rules'",
            )
        );
    }
}
