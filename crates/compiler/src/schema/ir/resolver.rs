use crate::core::types::*;
use crate::schema::ast::*;

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

    /// finds the TypeDefAst node for name at the latest version <= version.
    /// returns the raw AST node (unlike resolve_type) since generic templates
    /// are never registered in TypeRegistry
    pub fn resolve_type_def(&self, name: &str, version: i128) -> Option<(&'a TypeDefAst, i128)> {
        self.ast
            .versions
            .iter()
            .filter(|v| v.version <= version)
            .filter_map(|v| {
                v.blocks.iter().find_map(|block| match block {
                    VersionBlockAst::TypeDef(td) if td.name == name => Some((v.version, td)),
                    _ => None,
                })
            })
            .max_by_key(|(found_version, _)| *found_version)
            .map(|(found_version, td)| (td, found_version))
    }

    /// finds the TypeDefAst node for name at exactly version, used for
    /// extends Name@V chains which reference a specific version
    pub fn resolve_type_def_exact(&self, name: &str, version: i128) -> Option<&'a TypeDefAst> {
        self.ast.versions.iter().find_map(|v| {
            if v.version != version {
                return None;
            }
            v.blocks.iter().find_map(|block| match block {
                VersionBlockAst::TypeDef(td) if td.name == name => Some(td),
                _ => None,
            })
        })
    }
}
