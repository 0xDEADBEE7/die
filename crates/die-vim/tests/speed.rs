//! K15's budget: core's share of a keystroke, die-vim included, is at most 1 ms at p99 on a 10k-line
//! file. A key here is everything the host asks of core: the key, its turns, and the scene.

use die_test_cap as _;

use die_core::{Harness, Input, LineNumbers, parse_keys};
use die_vim::Vim;
use std::time::Instant;

/// Ordinary editing with a search highlight up: motions, typing, operators, undo and paste, in the
/// middle of the file, near its start and near its end.
const WORK: &str = "/let line_5000<CR>ofn typed(a, b) { a + b }<Esc>jjjkkwwwbbeex\
                    uddpyyPcwname<Esc>A;<Esc>0$ggjjjdwuGkkkiabc<Esc>u5000ggzzn";

/// Each key's ms, for the keys of `WORK` said over `rounds` times.
fn keystrokes_ms(rounds: usize) -> Vec<f64> {
    let text: String = (0..10_000)
        .map(|i| format!("    let line_{i} = compute(some_value, another_value) + {i};\n"))
        .collect();
    let mut h = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    h.set_window_size(100, 50);
    let (b, _) = h.open_text("t", &text).expect("a window");
    h.text_mut(b).unwrap().settings_mut().line_numbers = LineNumbers::Visual;
    let keys = parse_keys(WORK).expect("good notation");
    let mut ms = Vec::with_capacity(rounds * keys.len());
    for _ in 0..rounds {
        for key in &keys {
            let began = Instant::now();
            let errors = h.send(Input::Key(key.clone()));
            ms.push(began.elapsed().as_secs_f64() * 1e3);
            assert!(errors.is_empty(), "{errors:?}");
        }
    }
    ms
}

#[test]
#[ignore = "speed: runs in ci --full"]
fn a_keystroke_on_a_10k_line_file_costs_at_most_1ms_at_p99() {
    let mut ms = keystrokes_ms(40);
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q) as usize];
    let (p50, p99, most) = (at(0.5), at(0.99), at(1.0));
    println!(
        "{} keys: p50 {p50:.3} ms, p99 {p99:.3} ms, max {most:.3} ms",
        ms.len()
    );
    assert!(p99 <= 1.0, "p99 {p99:.3} ms is over 1 ms");
}
