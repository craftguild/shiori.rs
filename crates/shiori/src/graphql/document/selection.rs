use super::*;
use crate::rendering::Profile;
use chigiri::SelectedField;

pub(in crate::graphql) fn profile(context: FieldContext<'_, Context>) -> Result<Profile, Error> {
    let fields = context
        .selected_fields("Document")
        .map_err(|_| Error::Structure)?;
    let mut profile = Profile::default();
    for field in fields {
        match field.name() {
            // Font selection needs to derive the actual successful math dependencies.
            "fonts" => profile.structured.math = true,
            "nodes" => nodes(&mut profile, &field)?,
            _ => {}
        }
    }
    Ok(profile)
}

fn nodes(profile: &mut Profile, field: &SelectedField<'_>) -> Result<(), Error> {
    for kind in [
        "FencedCodeNode",
        "IndentedCodeNode",
        "MathBlockNode",
        "MathNode",
        "ImageNode",
    ] {
        for child in field.selected_fields(kind).map_err(|_| Error::Structure)? {
            match (kind, child.name()) {
                ("FencedCodeNode" | "IndentedCodeNode", "highlighting") => {
                    profile.structured.syntax = true
                }
                ("FencedCodeNode", "svg") => {
                    profile.structured.math = true;
                    profile.structured.mermaid = true;
                    profile.structured.plantuml = true;
                }
                ("MathBlockNode" | "MathNode", "svg") => profile.structured.math = true,
                ("ImageNode", "presentation") => profile.images = true,
                _ => {}
            }
        }
    }
    Ok(())
}
