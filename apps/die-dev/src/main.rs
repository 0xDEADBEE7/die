//! `die [--check a,b] [--handover socket] [--restart-after-frame] [--] [file...]`: your DIE
//! config, with the files you name. In DIE.app it's the one instance (`die::app`).

#[cfg(test)]
use die_test_cap as _;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match die_dev::parse_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("die: {e}");
            return ExitCode::from(2);
        }
    };
    let config = die_dev::config(args.files);
    let code = match (args.check, die::app::bundle()) {
        (Some(names), _) => die::check(config, names),
        (None, Some(bundle)) => die::app::run(config, bundle, args.launch),
        (None, None) => die::run(config),
    };
    ExitCode::from(code.clamp(0, 255) as u8)
}
