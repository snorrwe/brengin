# brengin

A small game engine in Rust, built on cecs ECS. Intended for renering large numbers of 2D sprites in 3D scenes.

Still experimental, pre-release.

## Features

- Plugins based architecture
- ECS
- Sprite rendering, sprite sheets
- Automatically pack textures in a texture atlas
- Immediate-mode UI
- Sync and async asset loading

## Getting started

### Requirements

- Nightly Rust
- Linux: `clang` + `mold` linker (see `.cargo/config.toml`), Vulkan, X11/Wayland libs
- Optional: Nix, `nix develop` gives dev shell with all dependencies on Linux
- Optional: [`just`](https://github.com/casey/just) for running examples

### Add to your project

```toml
[dependencies]
brengin = { git = "https://github.com/snorrwe/brengin" }
```

### Minimal example

```rust
use brengin::prelude::*;
use brengin::{App, DefaultPlugins};

fn setup(mut cmd: Commands) {
    // spawn camera, sprites, ...
}

fn main() {
    let mut app = App::default();
    app.add_plugin(DefaultPlugins);
    app.add_startup_system(setup);
    pollster::block_on(app.run()).unwrap();
}
```

## Examples

Run with `just <name>` or `cargo run --example <name>`.

| Example         | Shows                                  |
| --------------- | -------------------------------------- |
| `simple_sprite` |  animated sprite sheet    |
| `boids`         |  many entities, parallel systems |
| `mandelbrot`    |  custom WGSL shader       |
| `ui`            |  UI widgets               |

## Cargo features

| Feature        | Default | Description               |
| -------------- | ------- | ------------------------- |
| `audio`        | yes     | Audio support via `kira` |
| `tracing`      | yes     | `tracing` instrumentation |
| `assets-stats` | yes     | keep statistics about the loaded assets in a cecs Resource|

## Platform support

Linux Windows MacOS

## Development

- Dev shell: `nix develop`
- Tests: `cargo test`
- Logging: `RUST_LOG`

<!-- TODO: issues/PR expectations, or "personal project, not accepting contributions" -->

## License

MIT, see [LICENSE].
