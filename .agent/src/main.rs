use std::process::ExitCode;

/// The command line runs on a thread of its own with a stack that does not depend on
/// the platform: Windows gives the main thread 1 MiB (Linux and macOS 8 MiB), and the
/// unoptimised frames of the command dispatcher alone overflow that, so every `cv`
/// command of a debug build died before it parsed its arguments.
const STACK_BYTES: usize = 32 * 1024 * 1024;

fn main() -> ExitCode {
    match std::thread::Builder::new()
        .name("cv".to_owned())
        .stack_size(STACK_BYTES)
        .spawn(command_line)
    {
        // A panic has printed its message already.
        Ok(thread) => thread.join().unwrap_or(ExitCode::from(101)),
        Err(error) => {
            eprintln!("ccvl failed: cannot start the command thread: {error}");
            ExitCode::FAILURE
        }
    }
}

fn command_line() -> ExitCode {
    match careervector_cli::cli::run() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("ccvl failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}
