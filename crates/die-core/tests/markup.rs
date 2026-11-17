//! What a markup mode stands on (contract §8, §9, §5): a file-type mode that comes with its
//! files, decorations swapped as one change, and a call after the text has changed.

use die_test_cap as _;

use die_core::*;
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

/// A fresh directory for one test, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("die-core-markup-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Notes;
impl Mode for Notes {}

struct Ext;
impl Extension for Ext {
    fn name(&self) -> &'static str {
        "notes"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode("notes", Notes::default)
            .files(|p| p.extension().is_some_and(|e| e == "md"));
    }
}

fn has_mode(c: &mut Ctx, b: BufferId) -> bool {
    c.with_mode(b, |_: &mut Notes, _| ()).is_ok()
}

#[test]
fn a_file_type_mode_comes_with_its_files_and_goes_with_another_name() {
    let d = Dir::new("file-mode");
    let mut c = Ctx::new(vec![Box::new(Ext)]).unwrap();
    let md = c.open_file(&d.file("a.md", "# a\n")).unwrap();
    let txt = c.open_file(&d.file("b.txt", "b\n")).unwrap();
    let plain = c.create_text("*scratch*", "# not a file\n");
    assert!(has_mode(&mut c, md));
    assert!(!has_mode(&mut c, txt));
    assert!(!has_mode(&mut c, plain));

    // `:saveas` under a name that doesn't match takes it away, and one that does brings it.
    c.save_to(md, &d.0.join("a.txt"), false, true).unwrap();
    c.run_until_idle();
    assert!(!has_mode(&mut c, md));
    c.save_to(txt, &d.0.join("b.md"), false, true).unwrap();
    assert!(has_mode(&mut c, txt));
    // A mode enabled by hand on a buffer with no file stays.
    c.enable_mode::<Notes>(plain).unwrap();
    assert!(has_mode(&mut c, plain));
}

#[test]
fn on_edited_says_each_changed_buffer_once_after_what_was_ready() {
    let mut c = Ctx::new(vec![]).unwrap();
    let a = c.create_text("a", "one\n");
    let b = c.create_text("b", "two\n");
    let heard = Rc::new(RefCell::new(vec![]));
    let h = heard.clone();
    let sub = c.on_edited(move |ctx, buf| {
        h.borrow_mut()
            .push((buf, ctx.text(buf)?.rope().to_string()));
        Ok(())
    });
    c.run_until_idle();
    assert_eq!(*heard.borrow(), vec![], "nothing has changed yet");

    // Two edits, and one in a call queued behind them: one call, after all three.
    let t = c.text_mut(a).unwrap();
    t.tx(Origin::User).insert(0, "x").commit().unwrap();
    t.tx(Origin::Extension).insert(0, "y").commit().unwrap();
    c.defer(move |ctx| {
        ctx.text_mut(a)?.tx(Origin::User).insert(0, "z").commit()?;
        Ok(())
    });
    c.run_until_idle();
    assert_eq!(*heard.borrow(), vec![(a, "zyxone\n".to_string())]);

    // A decoration isn't a change of text, and a dropped handle hears nothing.
    c.text_mut(b)
        .unwrap()
        .decorate(0..1, Deco::Face("bold"))
        .unwrap();
    c.run_until_idle();
    assert_eq!(heard.borrow().len(), 1);
    drop(sub);
    c.text_mut(b)
        .unwrap()
        .tx(Origin::User)
        .insert(0, "x")
        .commit()
        .unwrap();
    c.run_until_idle();
    assert_eq!(heard.borrow().len(), 1);

    // A buffer killed before a subscriber's turn isn't told of to it.
    let _killer = c.on_edited(|ctx, buf| {
        ctx.kill(buf);
        Ok(())
    });
    let after = Rc::new(Cell::new(0));
    let n = after.clone();
    let _after = c.on_edited(move |_, _| {
        n.set(n.get() + 1);
        Ok(())
    });
    let t = c.text_mut(a).unwrap();
    t.tx(Origin::User).insert(0, "w").commit().unwrap();
    c.run_until_idle();
    assert!(!c.is_live(a));
    assert_eq!(after.get(), 0);
}

/// A repeat slower than its period is due again whenever the loop looks. The edit it makes is
/// told of in the turn that made it, and then the loop hands back for a frame: telling isn't a
/// way round "no second batch of timers until this has returned false".
#[test]
fn a_slow_repeat_that_edits_still_hands_the_loop_back_once_its_edit_is_told_of() {
    const MS: Duration = Duration::from_millis(1);
    let config = Config {
        clock: Clock::real(),
        ..Config::default()
    };
    let mut c = Ctx::with_config(vec![], config).unwrap();
    let b = c.create_text("a", "");
    let heard = Rc::new(Cell::new(0));
    let n = heard.clone();
    c.on_edited(move |_, _| {
        n.set(n.get() + 1);
        Ok(())
    })
    .detach();
    let repeat = c.every(MS, move |ctx| {
        sleep(3 * MS);
        let t = ctx.text_mut(b)?;
        t.tx(Origin::Extension).insert(0, "x").commit()?;
        Ok(())
    });
    sleep(2 * MS);
    let mut turns = 0;
    while c.step() && turns < 100 {
        turns += 1;
    }
    assert!(turns < 100, "the loop never handed back");
    assert_eq!(heard.get(), 1, "one firing, and its edit told of");
    drop(repeat);
}

#[test]
fn redecorate_swaps_decorations_as_one_change() {
    let mut c = Ctx::new(vec![]).unwrap();
    let b = c.create_text("a", "**bold** and `code`\n");
    let w = c.open_window(b, 40, 5).unwrap();
    let hide = || Deco::Hide {
        replacement: None,
        reveal: Reveal::Never,
    };
    let t = c.text_mut(b).unwrap();
    let first = t
        .redecorate(
            &[],
            vec![(0..2, hide()), (6..8, hide()), (2..6, Deco::Face("bold"))],
        )
        .unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(t.decorations().count(), 3);
    // The cursor was on the first `*`, which is hidden now.
    assert_eq!(t.cursor(w), Some(2));

    // A bad range anywhere changes nothing.
    let err = t.redecorate(&first, vec![(0..1, hide()), (5..99, hide())]);
    assert!(matches!(err, Err(Error::BadRange { start: 5, end: 99 })));
    assert_eq!(t.decorations().count(), 3);

    // The old ones go and the new ones come, dead ids skipped.
    let second = t
        .redecorate(&first, vec![(13..14, hide()), (18..19, hide())])
        .unwrap();
    assert_eq!(t.decorations().count(), 2);
    assert!(first.iter().all(|id| t.decoration(*id).is_none()));
    assert!(t.redecorate(&first, vec![]).unwrap().is_empty());
    assert_eq!(t.redecorate(&second, vec![]).unwrap(), vec![]);
    assert_eq!(t.decorations().count(), 0);
}
