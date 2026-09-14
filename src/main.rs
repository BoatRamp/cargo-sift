use std::ffi::OsString;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    let mut argv: Vec<OsString> = std::env::args_os().collect();
    // Invoked as a cargo subcommand (`cargo sift ...`), Cargo runs us as
    // `cargo-sift sift ...`. Drop that injected token so the flags parse flat,
    // while `cargo-sift ...` (run directly) keeps working too.
    if argv.get(1).is_some_and(|arg| arg == "sift") {
        argv.remove(1);
    }
    let args = cargo_sift::cli::Args::parse_from(argv);
    cargo_sift::run(args)
}
