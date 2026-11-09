//! Operator-defined ordering for media libraries and playlists.

use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPosition {
    Before,
    After,
}

/// Move the first matching dragged entry. Stale drags preserve the input order.
pub fn reorder_ids(
    ids: &[String],
    dragged_id: &str,
    target_id: &str,
    position: DropPosition,
) -> Vec<String> {
    let mut reordered = ids.to_vec();
    if dragged_id == target_id {
        return reordered;
    }
    let Some(from) = ids.iter().position(|id| id == dragged_id) else {
        return reordered;
    };
    let Some(target) = ids.iter().position(|id| id == target_id) else {
        return reordered;
    };
    let dragged = reordered.remove(from);
    let target = target - usize::from(from < target);
    reordered.insert(
        target + usize::from(position == DropPosition::After),
        dragged,
    );
    reordered
}

/// Apply saved IDs first, then keep newly scanned items in scan order.
pub fn apply_item_order<'a, T>(
    items: &'a [T],
    order: &[String],
    id_of: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    if order.is_empty() {
        return items.iter().collect();
    }
    let by_id: HashMap<_, _> = items.iter().map(|item| (id_of(item), item)).collect();
    let mut seen = HashSet::new();
    let mut ordered = Vec::with_capacity(items.len());
    for id in order {
        if let Some(item) = by_id.get(id.as_str())
            && seen.insert(id.as_str())
        {
            ordered.push(*item);
        }
    }
    ordered.extend(items.iter().filter(|item| !seen.contains(id_of(item))));
    ordered
}
