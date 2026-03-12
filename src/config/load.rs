use std::fs;
use std::path::Path;

use crate::config::external::{ExternalConfig, ExternalConfigSource, ExternalDocument};
use crate::config::internal::ActiveConfig;
use crate::error::Error;

pub fn read_source_document(source: &ExternalConfigSource) -> Result<ExternalDocument, Error> {
    match source {
        ExternalConfigSource::LocalFile { path } => read_local_file_document(path.as_str()),
        ExternalConfigSource::ClashSubscription { url } => Err(Error::unimplemented(format!(
            "loading Clash subscription source '{}' is not implemented yet; provide an ExternalDocument instead",
            url
        ))),
    }
}

pub fn read_local_file_document(path: impl AsRef<Path>) -> Result<ExternalDocument, Error> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)?;

    Ok(ExternalDocument::new(
        ExternalConfigSource::LocalFile {
            path: path.display().to_string(),
        },
        raw,
    ))
}

pub fn parse_local_document(document: &ExternalDocument) -> Result<ExternalConfig, Error> {
    let path = match &document.source {
        ExternalConfigSource::LocalFile { path } => path,
        ExternalConfigSource::ClashSubscription { url } => {
            return Err(Error::validation(format!(
                "Clash subscription document '{}' must be translated through the Clash adapter",
                url
            )));
        }
    };

    serde_yaml::from_str(document.raw.as_str()).map_err(|error| {
        Error::validation(format!("failed to parse local config '{}': {error}", path))
    })
}

pub fn load_local_document(document: &ExternalDocument) -> Result<ActiveConfig, Error> {
    ActiveConfig::from_external(parse_local_document(document)?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{load_local_document, read_source_document};
    use crate::config::external::{
        ExternalConfig, ExternalConfigSource, ListenerInput, ListenerProtocolInput, NodeInput,
        TargetRefInput,
    };
    use crate::error::Error;

    #[test]
    fn local_file_source_reads_and_loads_into_active_config() {
        let path = temp_config_path("local-file");
        let config = ExternalConfig {
            listeners: vec![ListenerInput {
                name: "local-socks".to_string(),
                bind: "127.0.0.1:1080".to_string(),
                protocol: ListenerProtocolInput::Socks5,
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
        let encoded = serde_yaml::to_string(&config).expect("config should serialize");
        fs::write(&path, encoded).expect("config file should be written");

        let document = read_source_document(&ExternalConfigSource::LocalFile {
            path: path.display().to_string(),
        })
        .expect("local file source should load");
        let active = load_local_document(&document).expect("local document should normalize");

        assert_eq!(active.listeners()[0].name, "local-socks");
        assert_eq!(active.nodes()[0].address, "1.1.1.1:443");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn clash_subscription_source_requires_a_preloaded_document() {
        let error = read_source_document(&ExternalConfigSource::ClashSubscription {
            url: "https://example.com/subscription".to_string(),
        })
        .expect_err("raw remote source loading is not implemented");

        assert_eq!(
            error,
            Error::unimplemented(
                "loading Clash subscription source 'https://example.com/subscription' is not implemented yet; provide an ExternalDocument instead",
            )
        );
    }

    fn temp_config_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("minibox-{label}-{nonce}.yaml"))
    }
}
