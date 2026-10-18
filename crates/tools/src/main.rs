use std::{io::Write, path::PathBuf};

use clap::Parser;

#[derive(Debug, clap_derive::Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Debug, Clone, clap_derive::Subcommand)]
enum Command {
    PackAssets {
        /// Path to the assets directory
        #[arg(default_value = "./assets")]
        assets_path: PathBuf,
        #[arg(default_value = "./assets.data")]
        out_path: PathBuf,
        /// Do not write checksum next to assets
        #[arg(long, default_value_t = false)]
        no_checksum: bool,
    },
}

fn main() {
    let args = Args::parse();

    match args.cmd {
        Command::PackAssets {
            assets_path,
            out_path,
            no_checksum,
        } => {
            let sh = xshell::Shell::new().unwrap();

            xshell::cmd!(sh, "tar -cf {out_path} -C {assets_path} .")
                .run()
                .expect("Failed to tar the assets");

            let checksum = xshell::cmd!(sh, "sha256sum {out_path}")
                .read()
                .expect("Failed to compute checksum");
            let (checksum, _) = checksum.split_once(char::is_whitespace).unwrap();

            if !no_checksum {
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(out_path.with_extension("checksum"))
                    .unwrap()
                    .write_all(checksum.as_bytes())
                    .unwrap()
            }

            println!(
                "{} written. SHA256 Checksum: {}",
                out_path.display(),
                checksum
            );
        }
    }
}
