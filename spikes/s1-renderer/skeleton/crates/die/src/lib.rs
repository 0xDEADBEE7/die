//! Stand-in for the `die` facade: die::run(config).
pub fn run() {
    let b = die_core::Buffer { text: "hello\nworld\n".into() };
    let mut st = die_vim::State { count: 1, reg: '"', pos: 0, hist: vec![] };
    let p = die_vim::dispatch(&mut st, &b, std::env::args().count() % 240, die_vim::Motion::Word(1));
    // keep the renderer linked: only runs when asked, so timing builds never opens a window
    if std::env::var("DIE_SKELETON_FIDELITY").is_ok() { let _ = die_render::fidelity::run(&[]); }
    println!("die skeleton {} {} {}", die_render::version(), die_term::term(), p);
}
