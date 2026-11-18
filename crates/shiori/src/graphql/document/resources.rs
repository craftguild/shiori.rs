use super::*;

object!(CodeHighlightingResolver, Node, parent;
    lines: Vec<Line> => (0..highlighting(parent)?.ok_or(Error::Structure)?.lines.len())
        .map(|index| Line { node: parent.clone(), index }).collect();
);
object!(CodeLineResolver, Line, parent;
    tokens: Vec<Token> => (0..parent.value()?.tokens.len()).map(|index| Token { line: parent.clone(), index }).collect();
);
object!(SyntaxTokenResolver, Token, parent;
    text: String => parent.value()?.text.clone();
    kind: generated::SyntaxTokenKind => token_kind(parent.value()?.kind);
);
object!(SvgContentResolver, Node, parent;
    markup: String => svg(parent)?.ok_or(Error::Structure)?.markup.clone();
    width: Option<f64> => svg(parent)?.ok_or(Error::Structure)?.width.map(f64::from);
    height: Option<f64> => svg(parent)?.ok_or(Error::Structure)?.height.map(f64::from);
    baseline: Option<f64> => svg(parent)?.ok_or(Error::Structure)?.baseline.map(f64::from);
    font_ids: Vec<Id> => ids(&svg(parent)?.ok_or(Error::Structure)?.font_ids);
);
object!(DocumentFontResolver, Font, parent;
    id: Id => parent.document.data.fonts[parent.index].id.clone().into();
    family: String => parent.document.data.fonts[parent.index].family.clone();
    mime_type: String => parent.document.data.fonts[parent.index].mime_type.clone();
    encoding: generated::FontEncoding => match parent.document.data.fonts[parent.index].encoding {
        oriha_core::FontEncoding::Base64 => generated::FontEncoding::Base64,
    };
    content: String => parent.document.data.fonts[parent.index].content.clone();
);
object!(DocumentDiagnosticResolver, Diagnostic, parent;
    severity: generated::DiagnosticSeverity => match parent.document.data.diagnostics[parent.index].severity {
        oriha_core::Severity::Info => generated::DiagnosticSeverity::Info,
        oriha_core::Severity::Warning => generated::DiagnosticSeverity::Warning,
        oriha_core::Severity::Error => generated::DiagnosticSeverity::Error,
    };
    code: String => parent.document.data.diagnostics[parent.index].code.clone();
    message: String => parent.document.data.diagnostics[parent.index].message.clone();
    node_id: Option<Id> => parent.document.data.diagnostics[parent.index].block_id.clone().map(Into::into);
    source_range: Option<oriha_core::SourceRange> => parent.document.data.diagnostics[parent.index].source_range;
    engine: generated::DocumentDiagnosticEngine => match parent.document.data.diagnostics[parent.index].engine {
        oriha_core::DiagnosticEngine::Core => generated::DocumentDiagnosticEngine::Core,
    };
);
object!(DocumentSourceRangeResolver, oriha_core::SourceRange, parent;
    start_line: i32 => parent.start_line.try_into().map_err(|_| Error::Structure)?;
    start_column: i32 => parent.start_column.try_into().map_err(|_| Error::Structure)?;
    end_line: i32 => parent.end_line.try_into().map_err(|_| Error::Structure)?;
    end_column: i32 => parent.end_column.try_into().map_err(|_| Error::Structure)?;
);
object!(AnnotationEntryResolver, AnnotationEntry, parent;
    key: String => parent.key.clone();
    value: String => parent.value.clone();
);
object!(ImagePresentationResolver, Arc<Image>, parent;
    url: String => parent.url.clone();
    original_url: String => parent.original_url.clone();
    candidates: Vec<Candidate> => parent.candidates.clone();
    sizes: Option<String> => parent.sizes.clone();
    width: Option<i32> => parent.width.map(i32::try_from).transpose().map_err(|_| Error::Structure)?;
    height: Option<i32> => parent.height.map(i32::try_from).transpose().map_err(|_| Error::Structure)?;
);
object!(ImageCandidateResolver, Candidate, parent;
    url: String => parent.url.clone();
    width: i32 => parent.width.try_into().map_err(|_| Error::Structure)?;
);

fn token_kind(value: oriha_core::TokenKind) -> generated::SyntaxTokenKind {
    use generated::SyntaxTokenKind as G;
    use oriha_core::TokenKind as T;
    match value {
        T::Plain => G::Plain,
        T::AttrName => G::AttrName,
        T::Builtin => G::Builtin,
        T::ClassName => G::ClassName,
        T::Comment => G::Comment,
        T::Constant => G::Constant,
        T::Deleted => G::Deleted,
        T::Doctype => G::Doctype,
        T::Function => G::Function,
        T::Inserted => G::Inserted,
        T::Keyword => G::Keyword,
        T::Number => G::Number,
        T::Operator => G::Operator,
        T::Property => G::Property,
        T::Punctuation => G::Punctuation,
        T::Section => G::Section,
        T::String => G::String,
        T::Symbol => G::Symbol,
        T::Tag => G::Tag,
        T::Url => G::Url,
        T::Variable => G::Variable,
    }
}
