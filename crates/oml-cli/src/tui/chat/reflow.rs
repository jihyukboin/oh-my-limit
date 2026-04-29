#[derive(Debug, Default)]
pub(super) struct TranscriptReflow {
    last_width: Option<u16>,
}

impl TranscriptReflow {
    pub(super) fn observe_width(&mut self, width: u16) -> bool {
        let changed = self.last_width.is_some_and(|last| last != width);
        self.last_width = Some(width);
        changed
    }
}
