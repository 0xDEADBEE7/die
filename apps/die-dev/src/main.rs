//! `die [--check a,b] [--] [file...]`: your DIE config, with the files you name.

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
    let code = match args.check {
        Some(names) => die::check(config, names),
        None => die::run(config),
    };
    ExitCode::from(code.clamp(0, 255) as u8)
}
