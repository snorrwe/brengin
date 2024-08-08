boids flags="":
    cargo r --example=boids {{flags}}


update:
    # update the index
    cargo update --dry-run
    cargo upgrade -i --offline
    nix flake update
