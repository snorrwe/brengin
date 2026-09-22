use super::*;

#[test]
fn test_align_left() {
    let bounds = dbg!(UiRect::from_pos_size(2, 3, 10, 10));
    let mut rect = dbg!(UiRect::from_pos_size(5, 5, 2, 2));

    let d = align_rect(
        &mut rect,
        &bounds,
        Some(HorizontalAlignment::Left),
        None,
        IVec2::X,
    );

    dbg!(&rect, &d);

    assert_eq!(rect.min_x, bounds.min_x + 1);
    assert_eq!(rect.max_x, bounds.min_x + 1 + rect.width());
    assert_eq!(rect.min_y, 4);
    assert_eq!(rect.max_y, 6);

    assert_eq!(d.x, -6);
    assert_eq!(d.y, 0);
}

#[test]
fn test_align_right() {
    let bounds = dbg!(UiRect::from_pos_size(2, 3, 20, 10));
    let mut rect = dbg!(UiRect::from_pos_size(5, 5, 2, 2));

    let d = align_rect(
        &mut rect,
        &bounds,
        Some(HorizontalAlignment::Right),
        None,
        IVec2::X,
    );

    dbg!(&rect, &d);

    assert_eq!(rect.min_x, bounds.max_x - 1 - 2);
    assert_eq!(rect.max_x, bounds.max_x - 1);
    assert_eq!(rect.min_y, 4);
    assert_eq!(rect.max_y, 6);

    assert_eq!(d.x, 5);
    assert_eq!(d.y, 0);
}

#[test]
fn test_align_center_horizontal() {
    let bounds = dbg!(UiRect::from_pos_size(2, 3, 20, 10));
    let mut rect = dbg!(UiRect::from_pos_size(-5, -5, 2, 2));

    let d = align_rect(
        &mut rect,
        &bounds,
        Some(HorizontalAlignment::Center),
        None,
        IVec2::X,
    );

    dbg!(&rect, &d);

    assert_eq!(rect.min_x, 1);
    assert_eq!(rect.max_x, 3);
    assert_eq!(rect.min_y, -6);
    assert_eq!(rect.max_y, -4);

    assert_eq!(d.x, 7);
    assert_eq!(d.y, 0);
}

#[test]
fn test_align_top() {
    let bounds = dbg!(UiRect::from_pos_size(2, 3, 10, 10));
    let mut rect = dbg!(UiRect::from_pos_size(5, 5, 2, 2));

    let d = align_rect(
        &mut rect,
        &bounds,
        None,
        Some(VerticalAlignment::Top),
        IVec2::Y,
    );

    dbg!(&rect, &d);

    assert_eq!(rect.min_x, 4);
    assert_eq!(rect.max_x, 6);
    assert_eq!(rect.min_y, -1);
    assert_eq!(rect.max_y, 1);

    assert_eq!(d.x, 0);
    assert_eq!(d.y, -5);
}

#[test]
fn test_align_bottom() {
    let bounds = dbg!(UiRect::from_pos_size(2, 3, 20, 10));
    let mut rect = dbg!(UiRect::from_pos_size(5, 5, 2, 2));

    let d = align_rect(
        &mut rect,
        &bounds,
        None,
        Some(VerticalAlignment::Bottom),
        IVec2::Y,
    );

    dbg!(&rect, &d);

    assert_eq!(rect.min_x, 4);
    assert_eq!(rect.max_x, 6);
    assert_eq!(rect.min_y, 5);
    assert_eq!(rect.max_y, 7);

    assert_eq!(d.x, 0);
    assert_eq!(d.y, 1);
}

#[test]
fn test_align_center_vertical() {
    let bounds = dbg!(UiRect::from_pos_size(2, 3, 20, 10));
    let mut rect = dbg!(UiRect::from_pos_size(-5, -5, 2, 2));

    let d = align_rect(
        &mut rect,
        &bounds,
        None,
        Some(VerticalAlignment::Center),
        IVec2::splat(100), // ignored
    );

    dbg!(&rect, &d);

    assert_eq!(rect.min_x, -6);
    assert_eq!(rect.max_x, -4);
    assert_eq!(rect.min_y, 2);
    assert_eq!(rect.max_y, 4);

    assert_eq!(d.x, 0);
    assert_eq!(d.y, 8);
}

#[test]
fn test_next_base_is_a_prefix_sum() {
    let mut base = WINDOW_LAYER;
    let mut bases = Vec::new();
    for h in [6u16, 10, 4] {
        bases.push(base);
        base = next_base(base, h);
    }

    assert_eq!(
        bases,
        vec![WINDOW_LAYER, WINDOW_LAYER + 6, WINDOW_LAYER + 16]
    );
    assert_eq!(base, WINDOW_LAYER + 20);
}

#[test]
fn test_next_base_saturates_below_context_layer() {
    assert_eq!(next_base(CONTEXT_LAYER - 10, 1000), CONTEXT_LAYER - 1);
    assert_eq!(next_base(u16::MAX - 1, 100), CONTEXT_LAYER - 1);
}

#[test]
fn test_window_order_assigns_distinct_increasing_z() {
    let mut order = WindowOrder::default();

    assert_eq!(order.z_of("a"), 0);
    assert_eq!(order.z_of("b"), 1);
    assert_eq!(order.z_of("c"), 2);
    // a known window keeps its index
    assert_eq!(order.z_of("a"), 0);
}

#[test]
fn test_window_order_apply_raise_moves_to_front() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");
    order.z_of("c");

    order.raise(0, "a");
    order.apply_raise();

    assert_eq!(order.z_of("b"), 0);
    assert_eq!(order.z_of("c"), 1);
    assert_eq!(order.z_of("a"), 2);
}

#[test]
fn test_window_order_raise_keeps_frontmost_candidate() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");
    order.z_of("c");

    // c is in front of a, so c wins whichever is requested first
    order.raise(2, "c");
    order.raise(0, "a");
    order.apply_raise();

    // raising the already frontmost window leaves the order untouched
    assert_eq!(order.z_of("a"), 0);
    assert_eq!(order.z_of("b"), 1);
    assert_eq!(order.z_of("c"), 2);
}

#[test]
fn test_window_order_raise_keeps_frontmost_candidate_reversed() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");
    order.z_of("c");

    order.raise(0, "a");
    order.raise(2, "c");
    order.apply_raise();

    assert_eq!(order.z_of("a"), 0);
    assert_eq!(order.z_of("b"), 1);
    assert_eq!(order.z_of("c"), 2);
}

#[test]
fn test_window_order_apply_raise_without_request_is_noop() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");

    order.apply_raise();

    assert_eq!(order.z_of("a"), 0);
    assert_eq!(order.z_of("b"), 1);
}

#[test]
fn test_window_order_apply_raise_consumes_the_request() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");

    order.raise(0, "a");
    order.apply_raise();
    // a is now in front; a second apply must not move anything
    order.apply_raise();

    assert_eq!(order.z_of("b"), 0);
    assert_eq!(order.z_of("a"), 1);
}

#[test]
fn test_window_order_retain_drawn_compacts_indices() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");
    order.z_of("c");

    order.order.retain(|name| (|name| name != "b")(name));

    assert_eq!(order.z_of("a"), 0);
    assert_eq!(order.z_of("c"), 1);
}

#[test]
fn test_window_order_raise_of_removed_window_is_ignored() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");

    // a window can be closed between the press and the raise being applied
    order.raise(0, "a");
    order.order.retain(|name| (|name| name != "a")(name));
    order.apply_raise();

    assert_eq!(order.z_of("b"), 0);
}

#[test]
fn test_window_order_iter_yields_back_to_front() {
    let mut order = WindowOrder::default();
    order.z_of("a");
    order.z_of("b");
    order.z_of("c");

    order.raise(0, "a");
    order.apply_raise();

    let names: Vec<&str> = {
        let this = &order;
        this.order.iter().map(|n| n.as_str())
    }
    .collect();
    assert_eq!(names, vec!["b", "c", "a"]);
}

#[test]
fn test_layer_in_band_covers_the_full_band() {
    let base = WINDOW_LAYER;
    let height = 6u16;

    assert!(layer_in_band(base, base, height));
    assert!(layer_in_band(base + height - 1, base, height));
    assert!(!layer_in_band(base - 1, base, height));
    assert!(!layer_in_band(base + height, base, height));
}

#[test]
fn test_layer_in_band_saturates_below_context_layer() {
    let base = CONTEXT_LAYER - 10;
    let height = 1000u16;

    // the band saturates at CONTEXT_LAYER - 1, so CONTEXT_LAYER itself is
    // never inside it even though the unsaturated band would have covered it
    assert!(!layer_in_band(CONTEXT_LAYER, base, height));
}

#[test]
fn test_window_order_contains() {
    let mut order = WindowOrder::default();

    let contains = |order: &WindowOrder, name: &str| order.order.iter().any(|n| n == name);

    assert!(!contains(&order, "a"));

    order.z_of("a");
    assert!(contains(&order, "a"));

    order.order.retain(|name| (|name| name != "a")(name));
    assert!(!contains(&order, "a"));
}
