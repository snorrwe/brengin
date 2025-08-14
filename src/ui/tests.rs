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
