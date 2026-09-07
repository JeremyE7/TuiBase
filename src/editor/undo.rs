#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub text: String,
    pub cursor: (usize, usize),
}

#[derive(Debug, Default)]
pub struct UndoManager {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl UndoManager {
    pub fn record(&mut self, before: Snapshot, after_text: &str) {
        if before.text == after_text {
            return;
        }
        self.undo.push(before);
        self.redo.clear();
        if self.undo.len() > 256 {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        Some(previous)
    }

    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::{Snapshot, UndoManager};

    fn snapshot(text: &str) -> Snapshot {
        Snapshot {
            text: text.to_owned(),
            cursor: (0, 0),
        }
    }

    #[test]
    fn undo_and_redo_restore_snapshots() {
        let mut history = UndoManager::default();
        history.record(snapshot("uno"), "dos");

        assert_eq!(history.undo(snapshot("dos")).unwrap().text, "uno");
        assert_eq!(history.redo(snapshot("uno")).unwrap().text, "dos");
    }
}
