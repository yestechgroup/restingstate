use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("OXC parse error")]
    OxcParse,

    #[error("Not a valid XState machine: {0}")]
    InvalidMachine(String),

    #[error("Unsupported XState feature: {0}")]
    UnsupportedFeature(String),

    #[error("Unsupported assign expression: {0}")]
    UnsupportedAssign(String),
}
