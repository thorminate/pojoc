use crate::ast::*;
use super::types::*;

#[derive(Debug)]
pub struct Resolver<'a> {
    pub ast: &'a SchemaAst,
}

impl<'a> Resolver<'a> {
    pub fn resolve_type(&self, name: &str, version: u32) -> Option<TypeId> {
        let exists = self.ast.versions.iter()
            .filter(|v| v.version <= version)
            .flat_map(|v| v.blocks.iter())
            .any(|block| match block {
                VersionBlockAst::TypeDef(td) => td.name == name,
                _ => false,
            });

        if exists {
            Some(TypeId { name: name.to_string(), version })
        } else {
            None
        }
    }

    pub fn is_primitive(&self, name: &str) -> bool {
        matches!(name,
            "byte" | "uchar" | "u8" | "ushort" | "u16" | "uint" | "u32" | "ulong" | "u64" |
            "char" | "i8" | "short" | "i16" | "int" | "i32" | "long" | "i64" |
            "float" | "f32" | "double" |  "f64" |
            "bool" | "boolean" |
            "string" | "str"
        )
    }
}