use thiserror::Error;

#[derive(Error, Debug)]
pub enum CodegenError {
    #[error("unknown type name: {0}")]
    UnknownType(String),
    #[error("invalid default value for field {field}: {msg}")]
    InvalidDefault { field: String, msg: String },
}
