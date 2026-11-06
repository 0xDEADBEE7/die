//! Window commands, which the Evil oracle's one-window cases can't see: the `C-w` map, `:sp :vs
//! :only :close`, and `:q` on a split, each as your Evil's source says (`evil-commands.el`).

use die_test_cap as _;

use die_core::{BufferId, Harness, WindowId};
use die_vim::Vim;
use std::path::Path;

/// A frame of 80 by 26, a window on `t`, and the keys typed. An error fails the test.
fn start(keys: &str) -> (Harness, BufferId, WindowId) {
    let mut h = Harness::new(vec![Box::new(Vim::default())]).expect("die-vim registers");
    let (b, w) = h.open_text("t", "one\ntwo\nthree\n").expect("a window");
    typed(&mut h, keys);
    (h, b, w)
}

fn typed(h: &mut Harness, keys: &str) {
    let log = h.type_keys(keys).expect("the keys parse");
    assert!(log.iter().all(|k| k.errors.is_empty()), "{keys}: {log:?}");
}

fn rows(h: &Harness, w: WindowId) -> usize {
    h.window_size(w).unwrap().1
}

fn cols(h: &Harness, w: WindowId) -> usize {
    h.window_size(w).unwrap().0
}

fn at(h: &Harness, w: WindowId) -> (usize, usize) {
    let p = h.placements().into_iter().find(|p| p.window == w).unwrap();
    (p.col, p.row)
}

#[test]
fn c_w_s_opens_above_takes_the_focus_and_evens_the_split() {
    for keys in ["<C-w>s", "<C-w>S", "<C-w><C-s>", ":sp<CR>", ":split<CR>"] {
        let (h, b, w) = start(keys);
        let list = h.window_list();
        assert_eq!(list.len(), 2, "{keys}");
        assert_eq!(list[1], w, "{keys}: the new window is above");
        assert_eq!(h.focused(), Some(list[0]), "{keys}: and has the focus");
        assert_eq!(h.window_buffer(list[0]), Some(b));
    }
    let (mut h, _, _) = start("<C-w>s");
    typed(&mut h, "<C-w>s");
    let sizes: Vec<_> = h.window_list().iter().map(|w| rows(&h, *w)).collect();
    assert!(
        sizes.iter().all(|r| r.abs_diff(sizes[0]) <= 1),
        "three even windows: {sizes:?}"
    );
}

#[test]
fn c_w_v_opens_on_the_left_and_takes_the_focus() {
    for keys in ["<C-w>v", "<C-w><C-v>", ":vs<CR>", ":vsplit<CR>"] {
        let (h, _, w) = start(keys);
        let list = h.window_list();
        assert_eq!(list[1], w, "{keys}");
        assert_eq!(at(&h, list[0]), (0, 0), "{keys}");
        assert_eq!(h.focused(), Some(list[0]), "{keys}");
    }
}

#[test]
fn a_count_sizes_the_new_window_and_nothing_evens_out() {
    let (h, _, _) = start("5<C-w>s");
    assert_eq!(rows(&h, h.focused().unwrap()), 4, "5 with its status line");
    let (h, _, _) = start("7<C-w>v");
    assert_eq!(cols(&h, h.focused().unwrap()), 6, "7 with its divider");
    let (h, _, _) = start(":5sp<CR>");
    assert_eq!(rows(&h, h.focused().unwrap()), 4);
}

#[test]
fn direction_keys_move_count_times_and_fail_at_the_edge() {
    let (mut h, _, bottom) = start("<C-w>s<C-w>s");
    let top = h.window_list()[0];
    typed(&mut h, "2<C-w>j");
    assert_eq!(h.focused(), Some(bottom));
    let log = h.type_keys("<C-w>j").unwrap();
    assert!(!log[1].errors.is_empty(), "no window below");
    for keys in ["<C-w>k<C-w>k", "<C-w><C-j><C-w><Down><C-w><Up><C-w><C-k>"] {
        typed(&mut h, keys);
        assert_eq!(h.focused(), Some(top), "{keys}");
    }
    let (mut h, _, right) = start("<C-w>v");
    let left = h.focused().unwrap();
    typed(&mut h, "<C-w>l");
    assert_eq!(h.focused(), Some(right));
    typed(&mut h, "<C-w>h");
    assert_eq!(h.focused(), Some(left));
}

#[test]
fn w_cycles_and_a_count_names_a_window_from_the_top_left() {
    let (mut h, _, _) = start("<C-w>s<C-w>v");
    let l = h.window_list();
    assert_eq!(l.len(), 3);
    assert_eq!(h.focused(), Some(l[0]));
    typed(&mut h, "<C-w>w");
    assert_eq!(h.focused(), Some(l[1]));
    typed(&mut h, "<C-w><C-w><C-w>w");
    assert_eq!(h.focused(), Some(l[0]), "it wraps");
    typed(&mut h, "<C-w>W");
    assert_eq!(h.focused(), Some(l[2]), "and back");
    typed(&mut h, "2<C-w>w");
    assert_eq!(h.focused(), Some(l[1]));
    typed(&mut h, "9<C-w>w");
    assert_eq!(h.focused(), Some(l[2]), "a count past the end is the last");
    typed(&mut h, "<C-w>t");
    assert_eq!(h.focused(), Some(l[0]));
    typed(&mut h, "<C-w>b");
    assert_eq!(h.focused(), Some(l[2]));
}

#[test]
fn p_goes_to_the_window_used_last_on_another_buffer() {
    let (mut h, b, _) = start("<C-w>v");
    let same = h.focused().unwrap();
    typed(&mut h, "<C-w>p");
    assert_eq!(h.focused(), Some(same), "every window shows this buffer");
    typed(&mut h, "<C-w>n");
    let new = h.focused().unwrap();
    assert_ne!(h.window_buffer(new), Some(b));
    typed(&mut h, "<C-w>p");
    assert_eq!(h.focused(), Some(same));
    typed(&mut h, "<C-w>p");
    assert_eq!(h.focused(), Some(new));
}

#[test]
fn n_and_vnew_show_a_new_empty_buffer_in_a_new_window() {
    for keys in ["<C-w>n", ":new<CR>", ":vne<CR>"] {
        let (h, b, w) = start(keys);
        let new = h.focused().unwrap();
        assert_ne!(new, w, "{keys}");
        let buf = h.window_buffer(new).unwrap();
        assert_ne!(buf, b, "{keys}");
        assert_eq!(h.buffer_name(buf), Some("*new*"), "{keys}");
        assert_eq!(h.window_buffer(w), Some(b), "{keys}");
    }
}

#[test]
fn close_goes_and_evens_out_a_split_that_stands() {
    for keys in ["<C-w>c", "<C-w><C-c>", ":clo<CR>", ":close<CR>"] {
        let (mut h, _, w) = start("<C-w>s");
        typed(&mut h, keys);
        assert_eq!(h.window_list(), [w], "{keys}");
        assert_eq!(h.focused(), Some(w), "{keys}");
        assert_eq!(rows(&h, w), 24, "{keys}: it has the frame back");
    }
    // Three windows, the top one small: closing the middle one evens out the two left.
    let (mut h, _, bottom) = start("<C-w>s3<C-w>s");
    let top = h.focused().unwrap();
    typed(&mut h, "<C-w>j<C-w>c");
    assert!(rows(&h, top).abs_diff(rows(&h, bottom)) <= 1);
}

#[test]
fn close_of_a_split_of_two_leaves_the_others_sizes_alone() {
    // A column of two beside one, with the right one's width changed: closing one of the column
    // takes the column's split with it, and Evil's balance on that does nothing.
    let (mut h, _, right) = start("<C-w>v<C-w>s5<C-w><lt>");
    let width = cols(&h, right);
    typed(&mut h, "<C-w>c");
    assert_eq!(cols(&h, right), width);
    assert_ne!(cols(&h, h.focused().unwrap()), width);
}

#[test]
fn the_only_window_cant_close() {
    let (mut h, _, w) = start("");
    let log = h.type_keys("<C-w>c").unwrap();
    assert!(!log[1].errors.is_empty());
    assert_eq!(h.window_list(), [w]);
}

#[test]
fn only_leaves_this_window() {
    for keys in ["<C-w>o", "<C-w><C-o>", ":on<CR>", ":only<CR>"] {
        let (mut h, _, _) = start("<C-w>s<C-w>v");
        let w = h.focused().unwrap();
        typed(&mut h, keys);
        assert_eq!(h.window_list(), [w], "{keys}");
        assert_eq!(rows(&h, w), 24, "{keys}");
    }
}

#[test]
fn q_on_a_split_closes_the_window_and_evens_out_its_split_as_c_w_c_does() {
    for keys in [":q<CR>", ":quit<CR>", ":q!<CR>", "<C-w>q"] {
        // Evil's `evil-quit` runs `evil-window-delete`, the same as `C-w c`: the two left share
        // the cells evenly, and neither takes them all.
        let (mut h, _, bottom) = start("3<C-w>s<C-w>j<C-w>s");
        let top = h.window_list()[0];
        typed(&mut h, keys);
        assert_eq!(h.window_list(), [top, bottom], "{keys}");
        assert!(rows(&h, top).abs_diff(rows(&h, bottom)) <= 1, "{keys}");
    }
}

#[test]
fn quit_on_the_last_window_asks_the_host_to_end() {
    let (mut h, _, w) = start("");
    typed(&mut h, "<C-w>q");
    assert!(h.quit_wanted());
    assert_eq!(h.window_list(), [w]);
}

#[test]
fn sizes_change_by_count_and_stop_at_the_frame() {
    let (mut h, _, bottom) = start("<C-w>s");
    let top = h.focused().unwrap();
    let was = rows(&h, top);
    typed(&mut h, "2<C-w>+");
    assert_eq!(rows(&h, top), was + 2);
    typed(&mut h, "<C-w>-");
    assert_eq!(rows(&h, top), was + 1);
    typed(&mut h, "5<C-w>_");
    assert_eq!(rows(&h, top), 4, "5 with its status line");
    typed(&mut h, "<C-w>_");
    assert_eq!(rows(&h, bottom), 1, "as high as it goes leaves one row");
    typed(&mut h, "<C-w>=");
    assert!(rows(&h, top).abs_diff(rows(&h, bottom)) <= 1);

    let (mut h, _, _) = start("<C-w>v");
    let left = h.focused().unwrap();
    let was = cols(&h, left);
    typed(&mut h, "3<C-w>>");
    assert_eq!(cols(&h, left), was + 3);
    typed(&mut h, "<C-w><lt>");
    assert_eq!(cols(&h, left), was + 2);
    typed(&mut h, "9<C-w>|");
    assert_eq!(cols(&h, left), 9, "the text is 9 wide, unlike a count on `C-w v`");

    let (mut h, _, w) = start("<C-w>+<C-w>>");
    typed(&mut h, "<C-w>_");
    assert_eq!(
        h.window_size(w),
        Some((80, 24)),
        "a sole window fills the frame"
    );
}

#[test]
fn moving_exchanging_and_rotating_keep_each_windows_place_in_the_tiling() {
    // H, J, K, L: the whole length of that side, the frame evened out.
    let (mut h, _, _) = start("<C-w>s<C-w>v");
    let w = h.focused().unwrap();
    typed(&mut h, "<C-w>L");
    assert_eq!(h.focused(), Some(w));
    assert_eq!(rows(&h, w), 24, "the whole height");
    assert_eq!(*h.window_list().last().unwrap(), w);
    typed(&mut h, "<C-w>K");
    assert_eq!(cols(&h, w), 80, "the whole width");
    assert_eq!(at(&h, w), (0, 0));

    // x: the focus keeps its place on the screen, on the other buffer, and each buffer keeps
    // its size.
    let (mut h, _, bottom) = start("5<C-w>s");
    let top = h.focused().unwrap();
    let (t, b) = (rows(&h, top), rows(&h, bottom));
    assert_ne!(t, b);
    typed(&mut h, "<C-w>x");
    assert_eq!(h.window_list(), [bottom, top]);
    assert_eq!(h.focused(), Some(bottom));
    assert_eq!(at(&h, bottom), (0, 0));
    assert_eq!((rows(&h, top), rows(&h, bottom)), (t, b));

    // Three in a split, the one between them keeping its size and its place.
    let (mut h, _, c) = start("<C-w>s3<C-w>s");
    let a = h.focused().unwrap();
    let m = h.window_list()[1];
    let (ra, rm, rc) = (rows(&h, a), rows(&h, m), rows(&h, c));
    assert_ne!(ra, rc);
    typed(&mut h, "3<C-w>x");
    assert_eq!(h.window_list(), [c, m, a]);
    assert_eq!((rows(&h, a), rows(&h, m), rows(&h, c)), (ra, rm, rc));

    // Beside each other, each keeps its text's width, and only the left place has a divider.
    let (mut h, _, right) = start("9<C-w>v");
    let left = h.focused().unwrap();
    let (l, r) = (cols(&h, left), cols(&h, right));
    typed(&mut h, "<C-w>x");
    assert_eq!((cols(&h, left), cols(&h, right)), (l, r));

    // In different splits a size swaps if the whole of it fits: your Evil's, read live, leaves
    // these at their places' heights, as the window in the column of two can't get a whole one.
    let (mut h, _, right) = start("<C-w>v<C-w>s");
    let upper = h.focused().unwrap();
    let lower = h.window_list()[1];
    let (u, d, r) = (rows(&h, upper), rows(&h, lower), rows(&h, right));
    typed(&mut h, "3<C-w>x");
    assert_eq!(h.window_list(), [right, lower, upper]);
    assert_eq!(h.focused(), Some(right));
    assert_eq!((rows(&h, right), rows(&h, lower), rows(&h, upper)), (u, d, r));

    // Two columns of two, where it does fit.
    let (mut h, _, _) = start("<C-w>v<C-w>s<C-w>l<C-w>s<C-w>h3<C-w>+");
    let l = h.window_list();
    let (a, b) = (rows(&h, l[0]), rows(&h, l[2]));
    assert_ne!(a, b);
    typed(&mut h, "3<C-w>x");
    assert_eq!(h.window_list(), [l[2], l[1], l[0], l[3]]);
    assert_eq!((rows(&h, l[0]), rows(&h, l[2])), (a, b));

    // r and R go round, and the focus keeps its place on the screen.
    let (mut h, _, _) = start("<C-w>s<C-w>s");
    let l = h.window_list();
    typed(&mut h, "<C-w>r");
    assert_eq!(h.window_list(), [l[2], l[0], l[1]]);
    assert_eq!(h.focused(), Some(l[2]));
    typed(&mut h, "<C-w>R");
    assert_eq!(h.window_list(), l);
    assert_eq!(h.focused(), Some(l[0]));
}

#[test]
fn a_split_on_a_file_finds_it_as_e_does() {
    let home = std::env::var_os("HOME").expect("a home directory");
    for ex in [":sp", ":vs", ":new", ":vne"] {
        let (h, _, _) = start(&format!("{ex} ~/die-v6b-no-such-file.txt<CR>"));
        let opened = h.window_buffer(h.focused().unwrap()).unwrap();
        assert_eq!(h.directory(opened), Some(Path::new(&home)), "{ex}");
    }
}

#[test]
fn digits_after_c_w_are_its_count_and_the_map_stays() {
    let (h, _, _) = start("<C-w>5s");
    assert_eq!(h.window_list().len(), 2);
    assert_eq!(rows(&h, h.focused().unwrap()), 4, "as `5 C-w s`");
    let (h, _, _) = start("<C-w>12v");
    assert_eq!(cols(&h, h.focused().unwrap()), 11, "digits add up");
    // And a macro's keys run the same.
    let (mut h, _, _) = start("qa<C-w>7vq");
    typed(&mut h, "<C-w>o@a");
    assert_eq!(cols(&h, h.focused().unwrap()), 6);
}

#[test]
fn c_w_with_control_arrows_moves_as_h_j_k_l_do() {
    let (mut h, _, bottom) = start("<C-w>s");
    let top = h.focused().unwrap();
    typed(&mut h, "<C-w><C-Down>");
    assert_eq!(h.focused(), Some(bottom));
    typed(&mut h, "<C-w><C-Up>");
    assert_eq!(h.focused(), Some(top));
    let (mut h, _, right) = start("<C-w>v");
    let left = h.focused().unwrap();
    typed(&mut h, "<C-w><C-Right>");
    assert_eq!(h.focused(), Some(right));
    typed(&mut h, "<C-w><C-Left>");
    assert_eq!(h.focused(), Some(left));
}
