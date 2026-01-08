use brengin::camera::{camera_bundle, PerspectiveCamera, WindowCamera};
use brengin::ui::{HorizontalAlignment, ScrollDescriptor, UiCoord, UiRoot, VerticalAlignment};
use brengin::{prelude::*, transform, CloseRequest};
use brengin::{App, DefaultPlugins};
use image::DynamicImage;

struct Label(String);

#[derive(Default)]
struct UiState {
    boid: Handle<DynamicImage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuState {
    Main,
    DragNDrop,
    Buttons,
    ImageGrid,
}

fn load_image(mut state: ResMut<UiState>, mut images: ResMut<Assets<DynamicImage>>) {
    let data = include_bytes!("./assets/boid.png");
    let image = image::load_from_memory(data).expect("Failed to load image");

    state.boid = images.insert(image);
}

fn image_grid(mut ctx: UiRoot, state: Res<MenuState>, ui_state: Res<UiState>) {
    let MenuState::ImageGrid = *state else { return };

    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoord::Percent(50),
            height: UiCoord::Percent(50),
            horizonal: HorizontalAlignment::Center,
            vertical: VerticalAlignment::Center,
        },
        |ui| {
            ui.grid(4, |ui| {
                for col in 0..4 {
                    ui.column(col, |ui| {
                        for row in 0..4 {
                            ui.context_menu(
                                |ui| {
                                    ui.margin(brengin::ui::Padding::vertical(10), |ui| {
                                        ui.image(
                                            ui_state.boid.clone(),
                                            UiCoord::Percent(50),
                                            UiCoord::Absolute(56),
                                        );
                                    });
                                },
                                |ui, _| {
                                    ui.allocate_area(128.into(), 128.into(), |ui| {
                                        ui.label(format!("col - {col} row - {row}"));
                                    });
                                },
                            );
                        }
                    });
                }
            });
        },
    );
}

fn back(mut ctx: UiRoot, mut state: ResMut<MenuState>) {
    if let MenuState::Main = *state {
        return;
    };
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoord::Absolute(100),
            height: UiCoord::Absolute(50),
            horizonal: HorizontalAlignment::Right,
            vertical: VerticalAlignment::Top,
        },
        |ui| {
            if ui.button("Back").inner.pressed {
                *state = MenuState::Main;
            }
        },
    );
}

fn menu(mut ctx: UiRoot, mut state: ResMut<MenuState>, cr: Res<CloseRequest>) {
    let MenuState::Main = *state else { return };
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoord::Absolute(500),
            height: UiCoord::Percent(50),
            horizonal: HorizontalAlignment::Center,
            vertical: VerticalAlignment::Center,
        },
        |ui| {
            ui.label("Choose example");
            if ui.button("Drag and drop").inner.pressed {
                *state = MenuState::DragNDrop;
            }
            if ui.button("Buttons").inner.pressed {
                *state = MenuState::Buttons;
            }
            if ui.button("ImageGrid").inner.pressed {
                *state = MenuState::ImageGrid;
            }
            if ui.button("Exit").inner.pressed {
                cr.request_close();
            }
        },
    );
}

struct Dnd {
    pub lists: Vec<Vec<usize>>,
}

fn dnd_ui(mut ctx: UiRoot, state: Res<MenuState>, mut dnd: ResMut<Dnd>, ui_state: Res<UiState>) {
    let MenuState::DragNDrop = *state else { return };

    let mut dropped_on = None;
    let mut dragged = None;
    for (i, list) in dnd.lists.iter_mut().enumerate() {
        ctx.window(
            brengin::ui::WindowDescriptor {
                name: format!("window {i}").as_str(),
                ..Default::default()
            },
            |ui| {
                ui.with_theme(
                    brengin::ui::Theme {
                        font_size: 24,
                        ..ui.theme().clone()
                    },
                    |ui| {
                        ui.label(format!("List {i}"));
                    },
                );

                for (j, n) in list.iter().enumerate() {
                    ui.drop_target(|ui, d| {
                        if d.dropped {
                            dropped_on = Some((i, j));
                        }
                        if ui
                            .drag_source(|ui, _| {
                                ui.label(format!("item {n}"));
                                ui.image(ui_state.boid.clone(), 32.into(), 32.into());
                            })
                            .is_being_dragged
                        {
                            dragged = Some((i, j));
                        }
                    });
                }
                if ui
                    .drop_target(|ui, _| {
                        ui.empty(128, 32);
                    })
                    .dropped
                {
                    dropped_on = Some((i, list.len()));
                }
            },
        );
    }
    if let (Some(dragged), Some(mut dropped_on)) = (dragged, dropped_on) {
        let n = dnd.lists[dragged.0].remove(dragged.1);
        dropped_on.1 = dropped_on.1.min(dnd.lists[dropped_on.0].len());
        dnd.lists[dropped_on.0].insert(dropped_on.1, n);
    }
}

#[derive(Debug, Default)]
struct FormState {
    pub data: String,
    pub pw: String,
}

const LOREM: &str = r#"
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
"#;

fn buttons_ui(mut ctx: UiRoot, mut label: ResMut<Label>, mut form: ResMut<FormState>) {
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoord::Percent(100),
            height: 300.into(),
            horizonal: HorizontalAlignment::Right,
            vertical: VerticalAlignment::Bottom,
        },
        |ui| {
            ui.scroll_area(
                ScrollDescriptor {
                    height: Some(UiCoord::Percent(100)),
                    width: None,
                },
                |ui| {
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
                },
            );
        },
    );
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoord::Percent(50),
            height: 400.into(),
            horizonal: HorizontalAlignment::Center,
            vertical: VerticalAlignment::Center,
        },
        |ui| {
            ui.with_theme_override(
                brengin::ui::ThemeOverride {
                    font_size: Some(24),
                    ..Default::default()
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

                    ui.horizontal(|ui| {
                        ui.label("username: ");
                        ui.input_string(&mut form.data);
                    });
                    ui.horizontal(|ui| {
                        ui.label("password: ");
                        ui.input_password(&mut form.pw);
                    });
                });

                // opening a context menu on-demand by user code
                let resp = ui.button("Open context menu");
                if resp.inner.pressed {
                    ui.open_context_menu(resp.id, None);
                }
                resp.context_menu(ui, |ui, s| {
                    ui.vertical(|ui| {
                        ui.label("hello");
                        if ui.button("close").inner.pressed {
                            s.open = false;
                        }
                    });
                });
            });
        },
    );
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoord::Absolute(500),
            height: 200.into(),
            horizonal: HorizontalAlignment::Left,
            vertical: VerticalAlignment::Top,
        },
        |ui| {
            ui.scroll_area(
                ScrollDescriptor {
                    width: Some(UiCoord::Percent(100)),
                    height: Some(UiCoord::Percent(100)),
                },
                |ui| {
                    ui.label(LOREM);
                },
            );
        },
    );

    // nested scroll area

    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoord::Absolute(400),
            height: 200.into(),
            horizonal: HorizontalAlignment::Left,
            vertical: VerticalAlignment::Center,
        },
        |ui| {
            ui.scroll_area(
                ScrollDescriptor {
                    width: Some(UiCoord::Percent(100)),
                    height: Some(UiCoord::Percent(100)),
                },
                |ui| {
                    ui.label("Nested scroll_area");
                    ui.scroll_area(
                        ScrollDescriptor {
                            width: Some(UiCoord::Percent(100)),
                            height: Some(UiCoord::Percent(100)),
                        },
                        |ui| {
                            ui.label(LOREM);
                        },
                    );
                },
            );
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
    app.insert_resource(MenuState::Main);
    app.insert_resource(UiState::default());
    app.insert_resource(Dnd {
        lists: vec![
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            vec![],
            vec![],
            vec![],
            vec![],
        ],
    });
    app.insert_resource(FormState::default());
    app.add_plugin(DefaultPlugins);
    app.add_startup_system(setup);
    app.add_startup_system(load_image);
    app.with_stage(brengin::Stage::Update, |s| {
        s.add_nested_stage(
            SystemStage::new("buttons-ui")
                .with_should_run(|state: Res<MenuState>| MenuState::Buttons == *state)
                .with_system(buttons_ui),
        );
        s.add_system(menu);
        s.add_system(dnd_ui);
        s.add_system(back);
        s.add_system(image_grid);
    });
    app.run().await.unwrap();
}

fn main() {
    tracing_subscriber::fmt::init();
    pollster::block_on(game());
}
