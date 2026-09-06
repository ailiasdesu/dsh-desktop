use dsh_native_helper::{serve, Helper};
use std::{io, path::PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("dsh-native-helper: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--cache")) {
        return Err("usage: dsh-native-helper --cache <owned-cache-directory>".into());
    }
    let cache = PathBuf::from(args.next().ok_or("missing cache directory")?);
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    let mut helper = Helper::open(&cache)?;
    // Coalesce binary frame headers and payload into one pipe write.
    serve(
        io::stdin().lock(),
        io::BufWriter::with_capacity(256 * 1024, io::stdout().lock()),
        &mut helper,
    )
}
