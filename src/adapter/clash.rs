use crate::config::external::ExternalDocument;
use crate::config::internal::ActiveConfig;
use crate::error::Error;

#[derive(Debug, Clone, Copy, Default)]
pub struct ClashLevelBAdapter;

impl ClashLevelBAdapter {
    pub fn supported_scope(&self) -> &'static str {
        "nodes + groups"
    }

    pub fn translate(&self, _document: &ExternalDocument) -> Result<ActiveConfig, Error> {
        Err(Error::unimplemented(
            "Clash translation is planned after the config foundation stage",
        ))
    }
}
