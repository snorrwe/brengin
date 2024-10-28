use crate::Plugin;

pub mod builder;
pub mod core;
pub mod rect;
pub mod text;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(builder::UiBuilderPlugin);
        app.add_plugin(core::UiCorePlugin);
    }
}
