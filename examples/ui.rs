use brengin::camera::{camera_bundle, PerspectiveCamera, WindowCamera};
use brengin::ui::{HorizontalAlignment, UiCoordinate, UiRoot, VerticalAlignment};
use brengin::{prelude::*, transform};
use brengin::{App, DefaultPlugins};

struct Label(String);

fn buttons_ui(mut ctx: UiRoot, mut label: ResMut<Label>) {
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoordinate::Percent(100),
            height: 300.into(),
            horizonal: HorizontalAlignment::Right,
            vertical: VerticalAlignment::Bottom,
        },
        |ui| {
            ui.scroll_vertical(None, |ui| {
                ui.grid(4, |cols| {
                    for col in 0..4 {
                        cols.column(col, |ui| {
                            for row in 0..10 {
                                let fill = row * 2;
                                let l = format!("{row} {col}\nPoggies{:s>fill$}", "");
                                if ui.button(&l).pressed() {
                                    label.0 = l;
                                }
                            }
                        });
                    }
                });
            });
        },
    );
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoordinate::Percent(50),
            height: 200.into(),
            horizonal: HorizontalAlignment::Center,
            vertical: VerticalAlignment::Center,
        },
        |ui| {
            ui.with_theme(
                brengin::ui::Theme {
                    font_size: 24,
                    ..ui.theme().clone()
                },
                |ui| {
                    ui.label("My panel is centered!!");
                },
            );
            ui.horizontal(|ui| {
                ui.label("Selected: ");
                ui.vertical(|ui| {
                    ui.label("This right here");
                    ui.label("|");
                    ui.label("|");
                    ui.label("v");
                    ui.label(&label.0);
                });
            });
        },
    );
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoordinate::Absolute(500),
            height: 200.into(),
            horizonal: HorizontalAlignment::Left,
            vertical: VerticalAlignment::Top,
        },
        |ui| {
            ui.scroll_vertical(None, |ui| {
            ui.scroll_horizontal(None, |ui| {
                ui.label(
                    r#"
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Mauris a velit nec purus dignissim consequat. Sed vel enim viverra, pellentesque tellus sed, laoreet purus. Morbi vehicula iaculis diam, at tempus ligula viverra aliquet.
Fusce vestibulum quis lectus ac feugiat. Donec ornare euismod felis id molestie. Proin ut enim nisl. Phasellus eu faucibus risus.
Quisque et sollicitudin ante, ut fringilla nisl. Nullam ut lacus purus. Mauris placerat vulputate egestas. Donec quam eros, cursus at sodales blandit, ultrices nec lorem. Donec nec turpis nisl.
Phasellus et efficitur orci, non fermentum est.

Donec a venenatis nibh, vitae auctor quam.
Nulla a fermentum nisl.
Maecenas feugiat faucibus urna, in suscipit est mollis a.
Ut vel nibh orci.
Suspendisse a ipsum elementum, bibendum odio in, aliquam leo.
Ut ac est non sem posuere dictum non ac neque.
Vestibulum ante lectus, dictum vitae metus at, rutrum ullamcorper elit.
In pulvinar, mauris ac ornare interdum, lectus lacus sodales libero, a pharetra libero nibh eu erat.
Quisque quis nulla egestas quam rhoncus mattis eget vitae est.
Vivamus lobortis sem sed odio porttitor, nec maximus sapien consequat.
Phasellus molestie nec arcu ut consequat.
Nunc convallis urna vitae leo mollis tempor.

Aliquam ligula mi, malesuada vitae gravida in, molestie eu tortor.
Integer vitae scelerisque nibh, pellentesque scelerisque enim.
In condimentum fermentum finibus.
Donec gravida odio a lobortis gravida.
In euismod, metus ut cursus mollis, velit neque iaculis orci, sed lacinia quam dui ac mauris.
Curabitur placerat rhoncus dui ut hendrerit.
Nullam ipsum libero, iaculis eu porta vel, imperdiet a velit.
Integer id purus velit.
Aliquam quam tortor, tincidunt non purus porttitor, sodales feugiat elit.
Fusce vitae viverra mauris.
Etiam gravida turpis quis turpis facilisis, in euismod mauris mollis.

Pellentesque habitant morbi tristique senectus et netus et malesuada fames ac turpis egestas.
Nulla venenatis non tellus ut lacinia.
Nulla cursus dolor vitae fermentum egestas.
In et augue ac metus commodo pellentesque.
Suspendisse blandit eros sit amet dolor commodo maximus.
Duis lobortis nunc sit amet ligula tempor dignissim sit amet id ipsum.
Donec vel risus sollicitudin, molestie ex quis, sagittis sem.
Praesent sed feugiat quam, eu tincidunt augue.
Maecenas orci nibh, molestie in porta nec, scelerisque eu justo.
Pellentesque faucibus sapien magna, vel posuere quam imperdiet quis.
Nunc rutrum metus elit, sit amet congue odio blandit pellentesque.
Maecenas nec sem et risus aliquam pretium in a sem.

Duis nec blandit augue, in commodo mauris.
Mauris rhoncus augue nec nibh vehicula, vel porttitor quam semper.
Donec mauris neque, efficitur id leo at, aliquet luctus est.
Duis nulla odio, ultricies quis efficitur sit amet, sagittis sed purus.
Vestibulum facilisis, nulla a volutpat convallis, diam metus lacinia mi, vel iaculis sem erat in sem.
Nam maximus lorem libero, ornare efficitur purus convallis ut.
Cras euismod velit sit amet mi pharetra bibendum.
Fusce sit amet nibh nec diam efficitur gravida eu a erat.
Curabitur quis semper nisl.
Aliquam eu ultricies orci.
Mauris ut pharetra orci.
Maecenas ac convallis ligula, id interdum turpis.
"#
                );
            });
            });
        },
    );
}

fn setup(mut cmd: Commands) {
    //camera
    cmd.spawn()
        .insert(WindowCamera)
        .insert_bundle(camera_bundle(PerspectiveCamera::default()))
        .insert_bundle(transform_bundle(transform::Transform::default()));
}

async fn game() {
    let mut app = App::default();
    app.insert_resource(Label(Default::default()));
    app.add_plugin(DefaultPlugins);
    app.add_startup_system(setup);
    app.with_stage(brengin::Stage::Update, |s| {
        s.add_system(buttons_ui);
    });
    app.run().await.unwrap();
}

fn main() {
    tracing_subscriber::fmt::init();
    pollster::block_on(game());
}
