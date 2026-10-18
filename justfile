export CARGO_PROFILE_RELEASE_DEBUG := "true"
export RUST_LOG := "warn,brengin=debug"
export WAYLAND_DISPLAY := ""

_default:
    @just --list

boids *args: (_example "boids" args)

sprites *args: (_example "sprites" args)

simple_sprite *args: (_example "simple_sprite" args)

mandelbrot *args: (_example "mandelbrot" args)

ui *args: (_example "ui" args)

tar_load *args: (_example "tar_load" args)

_example name *args:
    cargo r --example={{ name }} {{ args }}
