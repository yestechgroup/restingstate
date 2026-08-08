use crate::error::CodegenError;
use proc_macro2::TokenStream;
use xstate_ir::Machine;

pub fn generate(_machine: &Machine) -> Result<TokenStream, CodegenError> {
    Err(CodegenError::NotImplemented)
}
