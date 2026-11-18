//! Finite derivation options included in request coalescing and cache identities.
use oriha_core::StructuredOptions;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub structured: StructuredOptions,
    pub images: bool,
}
impl Profile {
    pub fn bits(self) -> u8 {
        u8::from(self.structured.syntax)
            | (u8::from(self.structured.math) << 1)
            | (u8::from(self.structured.mermaid) << 2)
            | (u8::from(self.structured.plantuml) << 3)
            | (u8::from(self.images) << 4)
    }
}
