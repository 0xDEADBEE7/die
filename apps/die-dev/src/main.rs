//! `die [--check a,b] [--handover socket] [--restart-after-frame] [--restart-loop] [--frontmost]
//! [--background] [--wait] [--] [file...]`: your DIE config, with the files you name and no
//! extension. In DIE.app it's the one instance, or from a terminal the client that sends files to
//! it (`die::app`). A config with extensions has this `main` with its own [`die_dev::Extra`].

#[cfg(test)]
use die_test_cap as _;

use std::process::ExitCode;

fn main() -> ExitCode {
    die_dev::main_with(die_dev::Extra::default)
}
