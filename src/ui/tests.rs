use super::*;

fn with_scroll_area(contents: impl FnOnce(&mut Ui)) {
    let mut world = World::new(0);
    let mut state = UiState::new();
    state.bounds = UiRect::from_pos_size(200, 150, 400, 300);
    state.viewport = state.bounds;
    state.scissors.push(state.bounds);
    world.insert_resource(state);
    world.insert_resource(UiIds::default());
    world.insert_resource(NextUiIds(Default::default()));
    world.insert_resource(TextTextureCache::default());
    world.insert_resource(Assets::<ShapingResult>::default());
    world.insert_resource(Assets::<OwnedTypeFace>::default());
    world.insert_resource(Theme::default());
    world.insert_resource(MouseInputs::default());
    world.insert_resource(KeyBoardInputs::default());
    world.insert_resource(UiMemory::default());
    world.insert_resource(DeltaTime(Duration::ZERO));
    world.insert_resource(Tick(0));
    world.insert_resource(NextUiInputs::default());
    world
        .run_system(|mut ui: Ui| {
            ui.scroll_area(
                ScrollDescriptor {
                    width: Some(UiCoord::Percent(100)),
                    height: Some(UiCoord::Percent(100)),
                },
                contents,
            );
        })
        .unwrap();
}

#[test]
fn test_scroll_grid_uses_viewport_width() {
    with_scroll_area(|ui| {
        ui.margin(
            Padding {
                left: Some(UiCoord::Absolute(50)),
                top: Some(UiCoord::Absolute(50)),
                ..Default::default()
            },
            |ui| {
                ui.grid(2, |cols| {
                    cols.label_span(.., "Room: 0, 0");
                    cols.end_row();
                    let mut bottom = 0;
                    for _ in 0..20 {
                        cols.label(0, "Level");
                        cols.column(1, |ui| {
                            let rect = ui.label("42").rect;
                            assert!(rect.min_x > 200 && rect.max_x < 400, "{rect:?}");
                            assert!(rect.min_y >= bottom, "{rect:?}");
                            bottom = rect.max_y;
                        });
                        cols.end_row();
                    }
                    assert!(bottom > 300);
                });
            },
        );
    });
}

#[test]
fn test_scroll_nested_horizontal_content_keeps_advancing() {
    with_scroll_area(|ui| {
        ui.horizontal(None, |ui| {
            let mut right = 0;
            for _ in 0..20 {
                let rect = ui.button("Wide button").rect;
                assert!(rect.min_x > right, "{rect:?}");
                right = rect.max_x;
            }
            assert!(right > 400);
        });
    });
}

#[test]
fn test_advance_layout_bounds_preserves_overflow_cursor() {
    let viewport = UiRect::from_pos_size(200, 150, 400, 300);
    let overflow = UiRect {
        min_x: -100,
        min_y: -100,
        max_x: 500,
        max_y: 400,
    };
    for (dir, expected) in [
        (
            LayoutDirection::TopDown(HorizontalAlignment::Left),
            UiRect {
                min_y: 400,
                max_y: 400,
                ..viewport
            },
        ),
        (
            LayoutDirection::BottomUp(HorizontalAlignment::Left),
            UiRect {
                min_y: -100,
                max_y: -100,
                ..viewport
            },
        ),
        (
            LayoutDirection::LeftRight(VerticalAlignment::Top),
            UiRect {
                min_x: 500,
                max_x: 500,
                ..viewport
            },
        ),
        (
            LayoutDirection::RightLeft(VerticalAlignment::Top),
            UiRect {
                min_x: -100,
                max_x: -100,
                ..viewport
            },
        ),
        (LayoutDirection::Center, viewport),
    ] {
        let mut bounds = viewport;
        advance_layout_bounds(&mut bounds, dir, overflow);
        assert_eq!(bounds, expected);
    }
}

#[test]
fn test_scroll_percentage_area_uses_finite_bounds() {
    with_scroll_area(|ui| {
        ui.allocate_area(
            AreaDescriptor {
                width: UiCoord::Percent(100),
                height: UiCoord::Absolute(200),
                scroll_on_overflow: false,
            },
            |ui| {
                assert_eq!(ui.ui_state.bounds.width(), 400);
                assert_eq!(ui.ui_state.bounds.height(), 200);
            },
        );
    });
}

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
