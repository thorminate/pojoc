use crate::ast::*;
use pojoc_core::types::*;

#[derive(Debug)]
pub struct Resolver<'a> {
    pub ast: &'a SchemaAst,
}

impl<'a> Resolver<'a> {
    pub fn resolve_type(&self, name: &str, version: i128) -> Option<TypeId> {
        self.ast
            .versions
            .iter()
            .filter(|v| v.version <= version)
            .filter(|v| {
                v.blocks.iter().any(|block| match block {
                    VersionBlockAst::TypeDef(td) => td.name == name,
                    _ => false,
                })
            })
            .map(|v| v.version)
            .max()
            .map(|found_version| TypeId {
                name: name.to_string(),
                version: found_version,
            })
    }
}
