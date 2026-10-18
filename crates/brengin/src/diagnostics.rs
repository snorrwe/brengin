use cecs::world_access;

use crate::prelude::*;

#[derive(Default, Clone)]
pub struct Diagnostics {
    pub entities: usize,
    pub archetypes: usize,
    pub staging_archetypes: usize,
    pub resources: usize,
}

pub struct DiagnosticsPlugin;

fn collect_stats_system(mut world: world_access::WorldAccess) {
    let w = world.world();
    let entities = w.num_entities();
    let archetypes = w.archetypes().len();
    let staging_archetypes = w.archetypes_staging().len();
    let resources = w.resources().len();

    world.world_mut().insert_resource(Diagnostics {
        entities,
        archetypes,
        staging_archetypes,
        resources,
    });
}

impl Plugin for DiagnosticsPlugin {
    fn build(self, app: &mut App) {
        app.insert_resource(Diagnostics::default());

        app.with_stage(Stage::PostUpdate, |s| {
            s.add_system(collect_stats_system);
        });
    }
}
