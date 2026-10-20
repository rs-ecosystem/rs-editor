use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use crate::schema::{Attrs, MarkType};

/// An inline annotation such as bold or a link.
#[derive(Clone)]
pub struct Mark {
    ty: MarkType,
    attrs: Attrs,
}

impl PartialEq for Mark {
    fn eq(&self, other: &Self) -> bool {
        self.ty == other.ty && (Arc::ptr_eq(&self.attrs, &other.attrs) || self.attrs == other.attrs)
    }
}

impl Eq for Mark {}

impl fmt::Debug for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.attrs.is_empty() {
            write!(f, "{}", self.ty.name())
        } else {
            write!(
                f,
                "{}({})",
                self.ty.name(),
                serde_json::Value::Object((*self.attrs).clone())
            )
        }
    }
}

impl Mark {
    pub(crate) fn new(ty: MarkType, attrs: Attrs) -> Self {
        Self { ty, attrs }
    }

    pub fn mark_type(&self) -> &MarkType {
        &self.ty
    }

    pub fn attrs(&self) -> &Attrs {
        &self.attrs
    }

    pub fn attr(&self, name: &str) -> Option<&serde_json::Value> {
        self.attrs.get(name)
    }

    /// Adds this mark to `set`, respecting rank order and exclusions.
    pub fn add_to_set(&self, set: &MarkSet) -> MarkSet {
        let mut copy: Option<Vec<Mark>> = None;
        let mut placed = false;
        for (index, other) in set.iter().enumerate() {
            if self == other {
                return set.clone();
            }
            if self.ty.excludes(&other.ty) {
                if copy.is_none() {
                    copy = Some(set[..index].to_vec());
                }
            } else if other.ty.excludes(&self.ty) {
                return set.clone();
            } else {
                if !placed && other.ty.rank() > self.ty.rank() {
                    copy.get_or_insert_with(|| set[..index].to_vec())
                        .push(self.clone());
                    placed = true;
                }
                if let Some(copy) = &mut copy {
                    copy.push(other.clone());
                }
            }
        }
        let mut copy = copy.unwrap_or_else(|| set.to_vec());
        if !placed {
            copy.push(self.clone());
        }
        MarkSet::from_sorted(copy)
    }

    pub fn remove_from_set(&self, set: &MarkSet) -> MarkSet {
        match set.iter().position(|mark| mark == self) {
            Some(index) => {
                let mut copy = set.to_vec();
                copy.remove(index);
                MarkSet::from_sorted(copy)
            }
            None => set.clone(),
        }
    }

    pub fn is_in_set(&self, set: &[Mark]) -> bool {
        set.iter().any(|mark| mark == self)
    }
}

/// A normalized, rank-ordered set of marks. Cheap to clone.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct MarkSet(Arc<[Mark]>);

impl fmt::Debug for MarkSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.iter()).finish()
    }
}

impl Deref for MarkSet {
    type Target = [Mark];

    fn deref(&self) -> &[Mark] {
        &self.0
    }
}

impl MarkSet {
    pub fn none() -> Self {
        Self(Arc::from([]))
    }

    /// Wraps marks that are already normalized.
    pub(crate) fn from_sorted(marks: Vec<Mark>) -> Self {
        Self(Arc::from(marks))
    }

    /// Builds a normalized set from arbitrary marks, applying exclusions.
    pub fn from_marks(marks: impl IntoIterator<Item = Mark>) -> Self {
        marks
            .into_iter()
            .fold(Self::none(), |set, mark| mark.add_to_set(&set))
    }

    pub fn same_set(&self, other: &MarkSet) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}
