//! `die [--check a,b] [--handover socket] [--restart-after-frame] [--restart-loop] [--frontmost]
//! [--background] [--wait] [--] [file...]`: your DIE config, with the files you name. In DIE.app it's the
//! one instance, or from a terminal the client that sends files to it (`die::app`).

#[cfg(test)]
use die_test_cap as _;

use std::process::ExitCode;

fn main() -> ExitCode {
    if let Some(code) = die::other_mode() {
        return ExitCode::from(code.clamp(0, 255) as u8);
    }
    die::render::latency::start_clock();
    let args = match die_dev::parse_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("die: {e}");
            return ExitCode::from(2);
        }
    };
    let code = match (args.check, die::app::bundle()) {
        (None, Some(bundle)) if die::app::is_client(&args.launch) => {
            die::app::client(&bundle, args.files, &args.launch)
        }
        (Some(names), _) => die::check(die_dev::config(args.files), names),
        (None, Some(bundle)) => die::app::run(die_dev::config(args.files), bundle, args.launch),
        (None, None) => die::run(die_dev::config(args.files)),
    };
    ExitCode::from(code.clamp(0, 255) as u8)
}
