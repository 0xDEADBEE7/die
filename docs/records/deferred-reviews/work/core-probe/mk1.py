import json, sys
W = 11  # tty 11 = 10 text columns
cases = [
    {"id": "ctl_at_end", "text": "aaaaaaaaa\u0001bb", "tty_width": W},
    {"id": "ctl_at_end2", "text": "aaaaaaaa\u0001bb", "tty_width": W},
    {"id": "ctl_after_space", "text": "aaaa bbbb\u0001cc", "tty_width": W},
    {"id": "c1_at_end", "text": "aaaaaaa\u0085bb", "tty_width": W},
    {"id": "c1_mid", "text": "aa\u0085bb", "tty_width": W},
    {"id": "del_at_end", "text": "aaaaaaaaa\u007fbb", "tty_width": W},
    {"id": "indent_long_word", "text": "    bbbbbbbbbbbb", "tty_width": W},
    {"id": "wide_after_space", "text": "aaaaaaa 中中", "tty_width": W},
    {"id": "wide_at_end", "text": "aaaaaaaaa中b", "tty_width": W},
    {"id": "wide_word_break", "text": "aaaa 中中中中", "tty_width": W},
    {"id": "full_then_nl", "text": "aaaaaaaaaa\nb\n", "tty_width": W},
    {"id": "full_eob", "text": "aaaaaaaaaa", "tty_width": W},
    {"id": "ws_full_then_nl", "text": "aaaa bbbb \nc", "tty_width": W},
    {"id": "spaces_past_end_nl", "text": "aaaa bbbb   \nc", "tty_width": W},
    {"id": "tab_huge", "text": "a\tbb", "tty_width": W, "tab": 20},
    {"id": "tab_at_row_start", "text": "aaaa bbbb\tcc", "tty_width": W, "tab": 4},
    {"id": "zwsp", "text": "aaaa​bbbbbbb", "tty_width": W},
    {"id": "combining_start", "text": "́abc", "tty_width": W},
    {"id": "word_then_spaces_eol", "text": "aaaaaaaaa      \nb", "tty_width": W},
]
json.dump(cases, open(sys.argv[1], "w"))
