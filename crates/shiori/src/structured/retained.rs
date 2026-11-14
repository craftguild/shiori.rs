use super::*;

trait Heap {
    fn heap(&self) -> usize;
}
impl Heap for String {
    fn heap(&self) -> usize {
        self.capacity()
    }
}
impl<T: Heap> Heap for Vec<T> {
    fn heap(&self) -> usize {
        self.capacity() * std::mem::size_of::<T>() + self.iter().map(Heap::heap).sum::<usize>()
    }
}
impl<T: Heap> Heap for Option<T> {
    fn heap(&self) -> usize {
        self.as_ref().map_or(0, Heap::heap)
    }
}
impl<T: Heap> Heap for Arc<T> {
    fn heap(&self) -> usize {
        std::mem::size_of::<T>() + 2 * std::mem::size_of::<usize>() + (**self).heap()
    }
}
impl Document {
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + self.root_ids.heap()
            + self.blocks.heap()
            + self.inlines.heap()
            + self.references.heap()
            + self.footnotes.heap()
            + self.anchors.heap()
    }
}
impl Heap for Block {
    fn heap(&self) -> usize {
        use StructuredBlockProperties::*;
        let properties = match &self.content {
            FencedCode { language, info, .. } | IndentedCode { language, info, .. } => {
                language.heap() + info.heap()
            }
            LinkReferenceDefinition { source } => source.heap(),
            List { marker, start, .. } => marker.heap() + start.heap(),
            ListItem { marker, .. } => marker.heap(),
            Callout { kind, title } => kind.heap() + title.heap(),
            ForeignHtml { origin, .. } => origin.heap(),
            GfmTable { alignments, .. } => {
                alignments.capacity() * std::mem::size_of::<oriha_core::BlockTableAlignment>()
            }
            _ => 0, // Recursive fields have been moved out during flattening.
        };
        self.id.heap()
            + self.text.heap()
            + self.child_ids.heap()
            + properties
            + self.rich_text_ids.heap()
            + self.header.heap()
            + self.rows.heap()
    }
}
impl Heap for Inline {
    fn heap(&self) -> usize {
        use RichTextProperties::*;
        let properties = match &self.content {
            Annotation {
                kind, id, classes, ..
            } => kind.heap() + id.heap() + classes.heap(),
            Emoji { shortcode } => shortcode.heap(),
            Link { url, title } | Image { url, title } => url.heap() + title.heap(),
            FootnoteReference { label } => label.heap(),
            _ => 0,
        };
        self.id.heap()
            + self.text.heap()
            + self.child_ids.heap()
            + properties
            + self.normalized_url.heap()
            + self.data.heap()
    }
}
impl Heap for Cell {
    fn heap(&self) -> usize {
        self.text.heap() + self.rich_text_ids.heap()
    }
}
impl Heap for Row {
    fn heap(&self) -> usize {
        self.cells.heap()
    }
}
impl Heap for Entry {
    fn heap(&self) -> usize {
        self.key.heap() + self.value.heap()
    }
}
impl Heap for Reference {
    fn heap(&self) -> usize {
        self.label.heap() + self.destination.heap() + self.title.heap()
    }
}
impl Heap for Footnote {
    fn heap(&self) -> usize {
        self.label.heap() + self.rich_text_ids.heap()
    }
}
