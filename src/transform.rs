use cecs::prelude::*;
use glam::{Quat, Vec3};

use crate::Plugin;

pub fn transform_bundle(tr: Transform) -> impl Bundle {
    (tr, GlobalTransform(tr))
}

pub fn spawn_child(
    parent: EntityId,
    cmd: &mut Commands,
    fun: impl FnOnce(&mut cecs::commands::EntityCommands),
) {
    let cmd = cmd.spawn();
    fun(cmd);
    cmd.insert_bundle((Parent(parent), AppendChild));
}

fn clean_children(mut q: Query<&mut Children>, exists: Query<&Parent>) {
    q.par_for_each_mut(|ch| {
        for i in (0..ch.len()).rev() {
            let id = ch[i];
            if !exists.contains(id) {
                ch.0.swap_remove(i);
            }
        }
    });
}

fn insert_missing_children(
    mut cmd: Commands,
    q: Query<&Parent, With<AppendChild>>,
    children: Query<EntityId, WithOut<Children>>,
) {
    for Parent(parent_id) in q.iter() {
        if children.contains(*parent_id) {
            cmd.entity(*parent_id).insert(Children(Default::default()));
        }
    }
}

fn append_new_children(
    mut cmd: Commands,
    q: Query<(EntityId, &Parent), With<AppendChild>>,
    mut children: Query<&mut Children>,
) {
    for (id, Parent(parent_id)) in q.iter() {
        // FIXME: add cecs command sentinel that ensures commands in
        // `insert_missing_children` execute before executing this system?
        if let Some(children) = children.fetch_mut(*parent_id) {
            children.0.push(id);
            cmd.entity(id).remove::<AppendChild>();
        }
    }
}

// parent id
struct AppendChild;

pub struct Parent(EntityId);

impl std::ops::Deref for Parent {
    type Target = EntityId;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Children(smallvec::SmallVec<[EntityId; 4]>);
unsafe impl Send for Children {}

impl std::ops::Deref for Children {
    type Target = [EntityId];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct GlobalTransform(pub Transform);

#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub pos: Vec3,
    pub scale: Vec3,
    pub rot: Quat,
}

impl Transform {
    pub fn compute_matrix(&self) -> glam::Mat4 {
        glam::Mat4::from_scale_rotation_translation(self.scale, self.rot, self.pos)
    }

    pub fn inverse(&self) -> Self {
        debug_assert!(self.rot.length() == 1.0);
        Self {
            pos: -self.pos,
            scale: 1.0 / self.scale,
            rot: self.rot.conjugate(),
        }
    }

    pub fn transform_point(&self, pos: Vec3) -> Vec3 {
        let pos = self.rot * pos;
        let pos = pos / self.scale;
        self.pos + pos
    }

    pub fn from_scale(scale: Vec3) -> Self {
        Self {
            scale,
            ..Default::default()
        }
    }

    pub fn from_position(pos: Vec3) -> Self {
        Self {
            pos,
            ..Default::default()
        }
    }

    pub fn from_rotation(rot: Quat) -> Self {
        Self {
            rot,
            ..Default::default()
        }
    }
}

impl<'a> std::ops::Mul<&'a Self> for Transform {
    type Output = Self;

    fn mul(self, rhs: &'a Self) -> Self::Output {
        let result = &self * rhs;
        result
    }
}

impl<'a> std::ops::Mul for &'a Transform {
    type Output = Transform;

    fn mul(self, rhs: Self) -> Self::Output {
        let mut result = *self;
        result.pos = self.pos + self.rot.mul_vec3(rhs.pos) * rhs.scale;
        result.scale *= rhs.scale;
        result.rot = self.rot.mul_quat(rhs.rot);
        result
    }
}

pub struct TransformPlugin;
impl Plugin for TransformPlugin {
    fn build(self, app: &mut crate::App) {
        app.stage(crate::Stage::PostUpdate)
            .add_system(insert_missing_children)
            .add_system(clean_children)
            .add_system(append_new_children.after(insert_missing_children));
        app.stage(crate::Stage::Transform)
            .add_system(update_root_transforms)
            .add_system(update_child_transforms);
    }
}

fn update_root_transforms(mut root: Query<(&Transform, &mut GlobalTransform), WithOut<Parent>>) {
    for (tr, global_tr) in root.iter_mut() {
        global_tr.0 = *tr;
    }
}

#[cfg(not(target_family = "wasm"))]
fn update_child_transforms(
    root: Query<(&Transform, &Children), WithOut<Parent>>,
    qchildren: Query<(&Transform, &mut GlobalTransform, Option<&Children>)>,
    pool: Res<JobPool>,
) {
    root.par_for_each(|(tr, children)| {
        pool.scope(|s| {
            children.chunks(256).for_each(|chunk| unsafe {
                let qchildren = &qchildren;
                let pool = &pool;
                s.spawn(move |_s| {
                    for child_id in chunk {
                        update_children_transforms_recursive(qchildren, tr, *child_id, pool)
                    }
                });
            });
        });
    });
}

#[cfg(not(target_family = "wasm"))]
unsafe fn update_children_transforms_recursive(
    qchildren: &Query<(&Transform, &mut GlobalTransform, Option<&Children>)>,
    parent_tr: &Transform,
    child_id: EntityId,
    pool: &JobPool,
) {
    let Some((transform, global_tr, children)) = qchildren.fetch_unsafe(child_id) else {
        // child may have been despawned
        return;
    };
    (*global_tr).0 = parent_tr * &*transform;
    let global_tr = &*global_tr;
    if let Some(children) = children {
        let children = (&*children).0.as_slice();
        pool.scope(move |s| {
            children.chunks(256).for_each(|children| {
                s.spawn(move |_s| {
                    for child_id in children {
                        update_children_transforms_recursive(
                            qchildren,
                            &global_tr.0,
                            *child_id,
                            pool,
                        );
                    }
                });
            });
        });
    }
}

#[cfg(target_family = "wasm")]
fn update_child_transforms(
    root: Query<(&Transform, &Children), WithOut<Parent>>,
    qchildren: Query<(&Transform, &mut GlobalTransform, Option<&Children>)>,
) {
    root.par_for_each(|(tr, children)| {
        children.iter().for_each(|child_id| unsafe {
            update_children_transforms_recursive(&qchildren, tr, *child_id);
        });
    });
}

#[cfg(target_family = "wasm")]
unsafe fn update_children_transforms_recursive(
    qchildren: &Query<(&Transform, &mut GlobalTransform, Option<&Children>)>,
    parent_tr: &Transform,
    child_id: EntityId,
) {
    let Some((transform, global_tr, children)) = qchildren.fetch_unsafe(child_id) else {
        // child may have been despawned
        return;
    };
    (*global_tr).0 = parent_tr * &*transform;
    let global_tr = &*global_tr;
    if let Some(children) = children {
        let children = (&*children).0.as_slice();
        children.iter().for_each(|child_id| {
            update_children_transforms_recursive(qchildren, &global_tr.0, *child_id);
        });
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            pos: Vec3::ZERO,
            scale: Vec3::splat(1.0),
            rot: Quat::default(),
        }
    }
}
