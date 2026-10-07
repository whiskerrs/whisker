//! Shared composition mechanics; platform modules choose identity and ordering.

use anyhow::{Result, ensure};
use std::{collections::BTreeMap, fmt::Debug};

pub(super) trait Merge {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()>;
}

macro_rules! public_merge {
    ($($ty:ty),+ $(,)?) => {$(
        impl $ty {
            /// Add a declaration atomically, retaining the original value on conflict
            ///
            /// Named objects compose by identity. Equal values are idempotent;
            /// conflicting values require explicit assignment by the caller.
            /// Ordered/opaque values are never interpreted as native instructions.
            /// Run ProjectIr::validate_structure after all contributions are applied.
            pub fn merge_from(&mut self, contribution: &Self) -> anyhow::Result<()> {
                let mut next = self.clone();
                $crate::project::merge::Merge::merge(&mut next, contribution, stringify!($ty))?;
                *self = next;
                Ok(())
            }
        }
    )+};
}
pub(super) use public_merge;

pub(super) fn equal<T: PartialEq>(a: &T, b: &T, path: &str) -> Result<()> {
    ensure!(a == b, "conflicting declaration at {path}");
    Ok(())
}

pub(super) fn optional<T: Clone + PartialEq>(
    a: &mut Option<T>,
    b: &Option<T>,
    path: &str,
) -> Result<()> {
    if let Some(b) = b {
        if let Some(a) = a {
            equal(a, b, path)?;
        } else {
            *a = Some(b.clone());
        }
    }
    Ok(())
}

pub(super) fn optional_object<T: Merge + Clone>(
    a: &mut Option<T>,
    b: &Option<T>,
    path: &str,
) -> Result<()> {
    if let Some(b) = b {
        if let Some(a) = a {
            a.merge(b, path)?;
        } else {
            *a = Some(b.clone());
        }
    }
    Ok(())
}

pub(super) fn values<K: Ord + Clone + Debug, V: Clone + PartialEq>(
    a: &mut BTreeMap<K, V>,
    b: &BTreeMap<K, V>,
    path: &str,
) -> Result<()> {
    for (key, value) in b {
        if let Some(existing) = a.get(key) {
            equal(existing, value, &format!("{path}[{key:?}]"))?;
        } else {
            a.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

pub(super) fn objects<K: Ord + Clone + Debug, V: Merge + Clone>(
    a: &mut BTreeMap<K, V>,
    b: &BTreeMap<K, V>,
    path: &str,
) -> Result<()> {
    for (key, value) in b {
        if let Some(existing) = a.get_mut(key) {
            existing.merge(value, &format!("{path}[{key:?}]"))?;
        } else {
            a.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

pub(super) fn append<T: Clone + PartialEq>(a: &mut Vec<T>, b: &[T]) {
    for value in b {
        if !a.contains(value) {
            a.push(value.clone());
        }
    }
}

pub(super) fn named<T: Clone + PartialEq, K: PartialEq + Debug>(
    a: &mut Vec<T>,
    b: &[T],
    key: impl Fn(&T) -> K,
    path: &str,
) -> Result<()> {
    for value in b {
        if let Some(existing) = a.iter().find(|entry| key(entry) == key(value)) {
            equal(existing, value, &format!("{path}[{:?}]", key(value)))?;
        } else {
            a.push(value.clone());
        }
    }
    Ok(())
}

impl Merge for serde_json::Value {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        if let (Self::Object(a), Self::Object(b)) = (&mut *self, other) {
            for (key, value) in b {
                if let Some(existing) = a.get_mut(key) {
                    existing.merge(value, &format!("{path}.{key}"))?;
                } else {
                    a.insert(key.clone(), value.clone());
                }
            }
            Ok(())
        } else {
            equal(self, other, path)
        }
    }
}
