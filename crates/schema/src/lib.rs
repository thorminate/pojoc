mod error;
mod ast;
mod lexer;
mod parser;

use pojoc_core::*;

#[derive(Debug)]
pub struct Schema {
    pub version: u64,
    pub fields: Vec<Field>
}

#[derive(Debug)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}
