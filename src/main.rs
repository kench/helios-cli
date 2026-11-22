//! `heliosctl`: see the library for what it does.

use std::process::ExitCode;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("heliosctl: can't start: {error}");
            return ExitCode::from(helios_cli::error::exit::OTHER);
        }
    };
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    ExitCode::from(runtime.block_on(helios_cli::cli::run(
        std::env::args_os(),
        &mut out,
        &mut err,
    )))
}
