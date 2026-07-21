#[derive(Debug, thiserror::Error)]
pub enum GovernorError {
    #[error("failed to read constitution at {path}: {source}")]
    ReadConstitution {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse constitution at {path}: {source}")]
    ParseConstitution {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
}
