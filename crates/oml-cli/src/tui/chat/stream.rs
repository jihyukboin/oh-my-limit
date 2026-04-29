#[derive(Debug, Default)]
pub(super) struct AssistantStream {
    committed: String,
    pending: String,
    active: bool,
}

impl AssistantStream {
    pub(super) fn start(&mut self) {
        self.committed.clear();
        self.pending.clear();
        self.active = true;
    }

    pub(super) fn push_delta(&mut self, delta: &str) {
        if !self.active {
            self.active = true;
        }
        self.pending.push_str(delta);
    }

    pub(super) fn finish_with(&mut self, source: String) {
        self.committed = source;
        self.pending.clear();
        self.active = true;
    }

    pub(super) fn clear(&mut self) {
        self.committed.clear();
        self.pending.clear();
        self.active = false;
    }

    pub(super) fn is_active(&self) -> bool {
        self.active
    }

    pub(super) fn active_cell(&self) -> Option<AssistantStreamingCell> {
        self.active
            .then(|| AssistantStreamingCell::new(self.visible_source()))
    }

    pub(super) fn take_finished_or_active(&mut self) -> Option<String> {
        if !self.active {
            return None;
        }

        self.active = false;
        self.commit_pending();
        Some(std::mem::take(&mut self.committed))
    }

    pub(super) fn commit_pending(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }

        self.committed.push_str(&self.pending);
        self.pending.clear();
        true
    }

    fn visible_source(&self) -> String {
        let mut source = String::with_capacity(self.committed.len() + self.pending.len());
        source.push_str(&self.committed);
        source.push_str(&self.pending);
        source
    }
}

#[derive(Debug)]
pub(super) struct AssistantStreamingCell {
    source: String,
}

impl AssistantStreamingCell {
    fn new(source: String) -> Self {
        Self { source }
    }

    pub(super) fn source(&self) -> &str {
        &self.source
    }
}
