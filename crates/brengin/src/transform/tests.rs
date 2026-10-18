use super::*;

#[test]
fn test_inverse() {
    let transform = Transform::from_position(Vec3::new(1.0, 2.0, 3.0))
        .with_rotation(Quat::from_rotation_x(0.12))
        .with_scale(Vec3::new(0.2, 0.1, 0.88));

    const EPSILON: f32 = 0.000001;

    for p in [
        Vec3::ZERO,
        Vec3::ONE,
        Vec3::NEG_ONE,
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(3.0, 2.0, 1.0),
        Vec3::new(3.0, 2.0, 0.0),
        Vec3::new(3.0, 0.0, 0.0),
    ] {
        let tr = transform.transform_point(p);
        let inv = transform.inverse_point(tr);

        let d = inv.distance(p);
        assert!(
            d <= EPSILON,
            "{inv} should be close to {p}. Diff: {d} Max diff: {EPSILON}"
        );
    }
}
