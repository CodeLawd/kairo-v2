use kairo_domain::media_order::{DropPosition, apply_item_order, reorder_ids};

#[test]
fn moves_before_or_after_and_ignores_stale_drags() {
    let ids = ["a", "b", "c"].map(String::from);
    assert_eq!(
        reorder_ids(&ids, "c", "a", DropPosition::Before),
        ["c", "a", "b"]
    );
    assert_eq!(
        reorder_ids(&ids, "a", "c", DropPosition::After),
        ["b", "c", "a"]
    );
    for (dragged, target) in [("a", "a"), ("z", "a"), ("a", "z")] {
        assert_eq!(reorder_ids(&ids, dragged, target, DropPosition::After), ids);
    }
}

#[test]
fn saved_order_keeps_known_items_and_appends_new_items() {
    let items = ["new", "b", "a"].map(String::from);
    let saved = ["a", "b", "gone", "a"].map(String::from);
    assert_eq!(
        apply_item_order(&items, &saved, String::as_str),
        [&items[2], &items[1], &items[0]]
    );
    assert_eq!(
        apply_item_order(&items, &[], String::as_str),
        [&items[0], &items[1], &items[2]]
    );
}
