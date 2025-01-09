export CARGO_PROFILE_RELEASE_DEBUG := "true"

_default:
    @just --list

boids flags="": (_example "boids" flags)

sprites flags="": (_example "sprites" flags)

mandelbrot flags="": (_example "mandelbrot" flags)

ui flags="": (_example "ui" flags)

update:
    # update the index
    cargo update --dry-run
    cargo upgrade -i --offline
    nix flake update

_example name flags:
    cargo r --example={{ name }} {{ flags }}
