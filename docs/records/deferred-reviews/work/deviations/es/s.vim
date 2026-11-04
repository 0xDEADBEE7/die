call cursor(1, 5)
exe "normal! obody\<Esc>"
call writefile([line('.') . ' ' . col('.') . ' ft=' . &ft . ' ai=' . &ai . ' inde=' . &indentexpr], 'pos.txt')
w! out.txt
qa!
