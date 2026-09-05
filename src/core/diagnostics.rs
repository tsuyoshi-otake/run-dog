//! In-memory diagnostics. Events are kinds and counts only.

const CAPACITY: usize = 32;

/// What happened, without payloads that can carry secrets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    UsageTick { files_touched: u16, more_work: bool },
    LimitFetch { remote_ok: bool },
    JsonlSkip { oversize: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticEvent {
    pub at_ms: u64,
    pub kind: DiagnosticKind,
}

/// Fixed-size ring. Oldest events are overwritten. Nothing is persisted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticRing {
    events: [Option<DiagnosticEvent>; CAPACITY],
    next: usize,
    len: usize,
}

impl Default for DiagnosticRing {
    fn default() -> Self {
        Self {
            events: [None; CAPACITY],
            next: 0,
            len: 0,
        }
    }
}

impl DiagnosticRing {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: DiagnosticEvent) {
        self.events[self.next] = Some(event);
        self.next = (self.next + 1) % CAPACITY;
        if self.len < CAPACITY {
            self.len += 1;
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = DiagnosticEvent> + '_ {
        let start = if self.len == CAPACITY { self.next } else { 0 };
        (0..self.len).filter_map(move |offset| self.events[(start + offset) % CAPACITY])
    }

    /// Privacy-safe copy for an explicit debug path. Kinds and counts only.
    #[must_use]
    pub fn snapshot(&self) -> Vec<DiagnosticEvent> {
        self.iter().collect()
    }
}

/// `RUNDOG_DIAGNOSTICS=1` enables the in-memory ring. Off by default.
#[must_use]
pub fn diagnostics_enabled() -> bool {
    matches!(std::env::var("RUNDOG_DIAGNOSTICS"), Ok(value) if value == "1")
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticEvent, DiagnosticKind, DiagnosticRing, CAPACITY};

    #[test]
    fn component_diagnostic_ring_drops_oldest_and_stores_no_payload_strings() {
        let mut ring = DiagnosticRing::new();
        assert!(ring.is_empty());
        for index in 0..(CAPACITY + 3) {
            ring.push(DiagnosticEvent {
                at_ms: index as u64,
                kind: DiagnosticKind::UsageTick {
                    files_touched: index as u16,
                    more_work: index % 2 == 0,
                },
            });
        }
        assert_eq!(ring.len(), CAPACITY);
        let first = ring.iter().next().expect("oldest kept after wrap");
        assert_eq!(first.at_ms, 3);
        let kinds: Vec<DiagnosticKind> = ring.iter().map(|event| event.kind).collect();
        assert!(matches!(
            kinds.last().copied(),
            Some(DiagnosticKind::UsageTick { .. })
        ));
        let encoded = format!("{kinds:?}");
        assert!(!encoded.contains("Bearer"));
        assert!(!encoded.contains("eyJ"));
        assert!(!encoded.contains("\\\\Users\\\\"));
        let snap = ring.snapshot();
        assert_eq!(snap.len(), CAPACITY);
        assert_eq!(snap[0].at_ms, 3);
    }
}
