//! Enrichment participates in the existing aggregate document output limit.
use super::*;
use oriha_core::DocumentNodeKind;
use std::io::{self, Write};

struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, value: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(value.len())
            .ok_or_else(|| io::Error::other("document output limit"))?;
        Ok(value.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Document {
    pub fn within_output_budget(&self) -> bool {
        let mut counter = Counter(MAX_BYTES);
        if serde_json::to_writer(&mut counter, &self.data).is_err() {
            return false;
        }
        // A shared lookup does not make repeated public image payloads free.
        for node in &self.data.nodes {
            if let DocumentNodeKind::Image { destination, .. } = &node.content
                && let Some(presentation) = self.images.get(destination)
                && serde_json::to_writer(&mut counter, &**presentation).is_err()
            {
                return false;
            }
        }
        true
    }
}
