use glam::Quat;

#[derive(Debug, Clone, Copy)]
pub enum PrimaryAxis {
    X,
    Y,
    Z,
}

pub trait RotationExtension {
    /// Rotate around a primary axis
    fn rotate_around_self(&self, axis: PrimaryAxis, angle: f32) -> Self;
}

impl RotationExtension for Quat {
    fn rotate_around_self(&self, axis: PrimaryAxis, angle: f32) -> Self {
        let (s, c) = (angle / 2.0).sin_cos();
        let [x, y, z, w] = self.to_array();
        let cx = c * x;
        let sx = s * x;

        let cy = c * y;
        let sy = s * y;

        let cz = c * z;
        let sz = s * z;

        let cw = c * w;
        let sw = s * w;

        match axis {
            PrimaryAxis::X => Quat::from_xyzw(cx + sw, cy + sz, cz - sy, cw - sx),
            PrimaryAxis::Y => Quat::from_xyzw(cx - sz, cy + sw, cz + sx, cw - sy),
            PrimaryAxis::Z => Quat::from_xyzw(cx + sy, cy + sx, cz + sw, cw - sz),
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    #[test]
    fn rotate_around_self_x_test() {
        let q0 = Quat::from_axis_angle(Vec3::Y, 1.0);

        let q1 = q0.mul_quat(Quat::from_axis_angle(Vec3::X, 2.0));
        let q2 = q0.rotate_around_self(PrimaryAxis::X, 2.0);

        assert_eq!(q1, q2);
    }

    #[test]
    fn rotate_around_self_y_test() {
        let q0 = Quat::from_axis_angle(Vec3::Y, 1.0);

        let q1 = q0.mul_quat(Quat::from_axis_angle(Vec3::Y, 2.0));
        let q2 = q0.rotate_around_self(PrimaryAxis::Y, 2.0);

        assert_eq!(q1, q2);
    }

    #[test]
    fn rotate_around_self_z_test() {
        let q0 = Quat::from_axis_angle(Vec3::Y, 1.0);

        let q1 = q0.mul_quat(Quat::from_axis_angle(Vec3::Z, 2.0));
        let q2 = q0.rotate_around_self(PrimaryAxis::Z, 2.0);

        assert_eq!(q1, q2);
    }
}
