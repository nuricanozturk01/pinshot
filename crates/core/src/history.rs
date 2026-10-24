//! Undo / redo of canvas operations: add, delete, move/resize/edit.
//!
//! Only geometry and text are tracked. Changing a colour or width is deliberately **not** an undo step.

use crate::annotate::Item;
use crate::geom::Point;

#[derive(Clone, Debug, PartialEq)]
pub struct Geom {
    pub pts: Vec<Point>,
    pub text: String,
}

impl Geom {
    pub fn of(it: &Item) -> Self {
        Geom {
            pts: it.pts.clone(),
            text: it.text.clone(),
        }
    }

    fn apply(&self, it: &mut Item) {
        it.pts = self.pts.clone();
        it.text = self.text.clone();
    }
}

#[derive(Clone, Debug)]
enum Entry {
    /// `out` is `None` while the item is in the list, `Some` while it is out (after an undo).
    Added {
        id: u64,
        index: usize,
        out: Option<Item>,
    },
    Deleted {
        id: u64,
        index: usize,
        out: Option<Item>,
    },
    Modified {
        id: u64,
        before: Geom,
        after: Geom,
    },
}

#[derive(Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn push(&mut self, e: Entry) {
        self.undo.push(e);
        self.redo.clear();
    }

    /// Call after the item was appended/inserted at `index`.
    pub fn added(&mut self, id: u64, index: usize) {
        self.push(Entry::Added {
            id,
            index,
            out: None,
        });
    }

    /// Call after the item was removed from `index`; the item is kept so undo can bring it back.
    pub fn deleted(&mut self, item: Item, index: usize) {
        let id = item.id;
        self.push(Entry::Deleted {
            id,
            index,
            out: Some(item),
        });
    }

    /// Record a geometry/text change. Does nothing when nothing actually changed.
    pub fn modified(&mut self, it: &Item, before: Geom) {
        let after = Geom::of(it);
        if before != after {
            self.push(Entry::Modified {
                id: it.id,
                before,
                after,
            });
        }
    }

    fn take(items: &mut Vec<Item>, id: u64) -> Option<Item> {
        let pos = items.iter().position(|i| i.id == id)?;
        Some(items.remove(pos))
    }

    fn put(items: &mut Vec<Item>, index: usize, item: Item) {
        items.insert(index.min(items.len()), item);
    }

    /// Undo one step; returns the id of the affected item.
    pub fn undo(&mut self, items: &mut Vec<Item>) -> Option<u64> {
        let e = self.undo.pop()?;
        let (id, redo) = match e {
            Entry::Added { id, index, .. } => {
                let out = Self::take(items, id);
                (id, Entry::Added { id, index, out })
            }
            Entry::Deleted { id, index, out } => {
                if let Some(item) = out {
                    Self::put(items, index, item);
                }
                (
                    id,
                    Entry::Deleted {
                        id,
                        index,
                        out: None,
                    },
                )
            }
            Entry::Modified { id, before, after } => {
                if let Some(it) = items.iter_mut().find(|i| i.id == id) {
                    before.apply(it);
                }
                (id, Entry::Modified { id, before, after })
            }
        };
        self.redo.push(redo);
        Some(id)
    }

    pub fn redo(&mut self, items: &mut Vec<Item>) -> Option<u64> {
        let e = self.redo.pop()?;
        let (id, undo) = match e {
            Entry::Added { id, index, out } => {
                if let Some(item) = out {
                    Self::put(items, index, item);
                }
                (
                    id,
                    Entry::Added {
                        id,
                        index,
                        out: None,
                    },
                )
            }
            Entry::Deleted { id, index, .. } => {
                let out = Self::take(items, id);
                (id, Entry::Deleted { id, index, out })
            }
            Entry::Modified { id, before, after } => {
                if let Some(it) = items.iter_mut().find(|i| i.id == id) {
                    after.apply(it);
                }
                (id, Entry::Modified { id, before, after })
            }
        };
        self.undo.push(undo);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::Kind;
    use crate::color::Rgba;

    fn rect(x: f32) -> Item {
        Item::new(
            Kind::Rect,
            Rgba::BLACK,
            3,
            vec![Point::new(x, 0.0), Point::new(x + 10.0, 10.0)],
        )
    }

    #[test]
    fn add_undo_redo_keeps_property_edits() {
        let mut items = vec![];
        let mut h = History::default();
        let it = rect(0.0);
        let id = it.id;
        items.push(it);
        h.added(id, 0);
        items[0].width = 9; // a property edit: not an undo step
        assert_eq!(h.undo(&mut items), Some(id));
        assert!(items.is_empty());
        assert_eq!(h.redo(&mut items), Some(id));
        assert_eq!(
            items[0].width, 9,
            "redo brings back the item as it was, including the new width"
        );
        assert!(h.undo(&mut items).is_some() && items.is_empty());
    }

    #[test]
    fn modification_and_property_changes_are_separate() {
        let mut items = vec![rect(0.0)];
        let mut h = History::default();
        h.added(items[0].id, 0);
        let before = Geom::of(&items[0]);
        items[0].translate(5.0, 5.0);
        h.modified(&items[0], before);
        items[0].width = 11; // not tracked
        h.undo(&mut items);
        assert_eq!(items[0].pts[0], Point::new(0.0, 0.0));
        assert_eq!(items[0].width, 11);
        h.undo(&mut items);
        assert!(items.is_empty());
        h.redo(&mut items);
        h.redo(&mut items);
        assert_eq!(items[0].pts[0], Point::new(5.0, 5.0));
    }

    #[test]
    fn delete_undo_restores_position_in_the_list() {
        let mut items = vec![rect(0.0), rect(20.0), rect(40.0)];
        let ids: Vec<u64> = items.iter().map(|i| i.id).collect();
        let mut h = History::default();
        let removed = items.remove(1);
        h.deleted(removed, 1);
        h.undo(&mut items);
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), ids);
        h.redo(&mut items);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn a_new_action_clears_redo_and_no_op_modification_is_ignored() {
        let mut items = vec![rect(0.0)];
        let mut h = History::default();
        h.added(items[0].id, 0);
        h.undo(&mut items);
        assert!(h.can_redo());
        let n = rect(50.0);
        h.added(n.id, 0);
        items.push(n);
        assert!(!h.can_redo());
        let before = Geom::of(&items[0]);
        h.modified(&items[0], before);
        assert!(h.undo(&mut items).is_some());
        assert!(
            !h.can_undo(),
            "the unchanged 'modification' did not add a step"
        );
    }
}
