fn main() {
    println!("cargo:rerun-if-changed=assets");

    let sh = xshell::Shell::new().unwrap();
    xshell::cmd!(sh, "brengin-tools pack-assets assets assets.data")
        .run()
        .unwrap();
}
