export CARGO_PROFILE_RELEASE_DEBUG:="true"

boids flags="":
    cargo r --example=boids {{flags}}

sprites flags="":
    cargo r --example=sprites {{flags}}


update:
    # update the index
    cargo update --dry-run
    cargo upgrade -i --offline
    nix flake update
