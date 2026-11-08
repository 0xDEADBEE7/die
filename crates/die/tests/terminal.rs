//! A terminal-like surface in the DIE you run, with die-vim and the facade's DIE layer (scope
//! D18): in insert state every key die-vim's insert state binds in text reaches it, the leader
//! too, while Cmd-V pastes and Cmd-Q quits. Esc goes to normal state, where die-vim's keys for
//! text do nothing to it.

use die_test_cap as _;

use die::core::*;
use die::{Config, start};
use std::cell::RefCell;
use std::rc::Rc;

struct Term(Rc<RefCell<Vec<Event>>>);

impl Surface for Term {
    fn kind(&self) -> &'static str {
        "term"
    }
    fn render(&mut self, frame: &mut Frame, _: &RenderInfo) {
        frame.text(0, 0, "$ ", &FrameCell::blank());
    }
    fn event(&mut self, ev: &Event, _: &mut Ctx) -> bool {
        self.0.borrow_mut().push(ev.clone());
        true
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
}

fn editor() -> (Ctx, BufferId, Rc<RefCell<Vec<Event>>>) {
    let mut ctx = start(Config::default()).unwrap();
    let seen = Rc::new(RefCell::new(vec![]));
    let buf = ctx.create_surface("term", Term(seen.clone()));
    ctx.show(buf, ShowIn::Current).unwrap();
    ctx.run_until_idle();
    seen.borrow_mut().clear();
    (ctx, buf, seen)
}

/// The first key of every binding die-vim's insert state has in text.
fn insert_keys(ctx: &Ctx) -> Vec<KeyEvent> {
    let mut out: Vec<KeyEvent> = vec![];
    for name in ctx.command_names() {
        let d = ctx.describe_command(name).unwrap();
        let bound = d
            .keys
            .iter()
            .filter(|(map, state, _)| map == "kind text" && *state == State::Insert);
        for (.., keys) in bound {
            let Ok(pats) = parse_pats(keys) else { continue };
            if let Some(Pat::Chord(c)) = pats.first() {
                let ev = KeyEvent::press(c.key).mods(c.mods);
                if !out.contains(&ev) {
                    out.push(ev);
                }
            }
        }
    }
    out
}

#[test]
fn in_insert_state_die_vims_insert_keys_and_the_leader_reach_the_terminal() {
    let (mut ctx, _, seen) = editor();
    let mut keys = insert_keys(&ctx);
    let ctrl: Vec<_> = keys.iter().filter(|k| k.mods.ctrl).collect();
    assert!(
        ctrl.len() >= 20,
        "die-vim binds C-a to C-y in insert: {ctrl:?}"
    );
    keys.retain(|k| k.key != Key::Esc);
    keys.push(KeyEvent::press(Key::Char('\\')));
    for k in &keys {
        ctx.input(k.clone());
        ctx.run_until_idle();
    }
    let got: Vec<KeyEvent> = seen
        .borrow()
        .iter()
        .filter_map(|e| match e {
            Event::Key(k) => Some(k.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(got, keys);
    assert_eq!(ctx.state(), State::Passthrough);
    assert!(
        ctx.messages.iter().all(|m| !m.contains("undefined")),
        "{:?}",
        ctx.messages
    );
}

#[test]
fn cmd_v_pastes_and_cmd_q_quits() {
    let (mut ctx, _, seen) = editor();
    ctx.set_clipboard(FakeClipboard::default());
    let cmd = Mods {
        super_: true,
        ..Mods::default()
    };
    ctx.set_register(
        '"',
        Register {
            kind: RegKind::Char,
            text: "git status".into(),
        },
    );
    ctx.input(KeyEvent::press(Key::Char('v')).mods(cmd));
    ctx.run_until_idle();
    assert_eq!(*seen.borrow(), [Event::Paste("git status".into())]);
    let d = ctx
        .describe_key(&[KeyEvent::press(Key::Char('v')).mods(cmd)])
        .unwrap();
    assert!(
        d.to_string()
            .starts_with("<D-v> in Insert runs surface-paste"),
        "{d}"
    );
    ctx.input(KeyEvent::press(Key::Char('q')).mods(cmd));
    ctx.run_until_idle();
    assert!(ctx.quit_wanted());
}

#[test]
fn in_normal_state_die_vims_text_keys_leave_the_terminal_alone() {
    let (mut ctx, buf, seen) = editor();
    ctx.type_keys("<Esc>");
    assert_eq!(ctx.state(), State::Normal);
    for keys in [
        "jjkk", "dd", "x", "u", "<C-r>", "p", "yy", "v", "ciwfoo", "gg", "G", "/x<CR>",
    ] {
        ctx.type_keys(keys);
    }
    assert!(ctx.is_live(buf));
    assert!(ctx.text(buf).is_err(), "still a surface");
    assert!(
        ctx.messages.iter().all(|m| !m.contains("panicked")),
        "{:?}",
        ctx.messages
    );
    let pasted = seen.borrow().iter().any(|e| matches!(e, Event::Paste(_)));
    assert!(!pasted, "die-vim's p is text's, not the terminal's");
}
