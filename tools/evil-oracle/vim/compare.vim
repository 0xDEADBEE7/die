" compare.vim: replay each fixture case in textbook Vim and compare
" the result with Evil's.  A dev aid for flagging deviations; the
" fixtures never depend on it.
"
"   vim --not-a-term -u NONE -i NONE -N -n -S vim/compare.vim \
"       -c 'call CompareFixture("fixtures/x.json", "out.jsonl")' -c 'qa!'
"
" Vim runs with -u NONE and only the options below, which match the
" user's Evil where an option exists (shiftwidth 4, spaces, autoindent,
" smartcase).  Each case writes one JSON line: Vim's result, and the
" fields where it differs from Evil's.  Errors are compared by count:
" Vim's v:errmsg (at most one per case) plus its beeps (debug=beep),
" against the length of Evil's errors.

set nocompatible
set shiftwidth=4 softtabstop=0 expandtab autoindent
set ignorecase smartcase
set backspace=indent,eol,start
set clipboard=
set nomore noswapfile
set laststatus=0 noruler noshowcmd
set debug=beep

function! s:Keys(tokens) abort
  let keys = ''
  for t in a:tokens
    if len(t) > 1 && t[0] ==# '<'
      let keys .= eval('"\' . t . '"')
    else
      let keys .= t
    endif
  endfor
  return keys
endfunction

" Turn a Vim-notation string such as "A!<Esc>j" into keys.
function! s:Notation(text) abort
  return substitute(a:text, '<\([A-Za-z][-A-Za-z]*\)>',
        \ '\=eval(''"\<'' . submatch(1) . ''>"'')', 'g')
endfunction

function! s:RegType(t) abort
  if a:t ==# 'v' | return 'char' | endif
  if a:t ==# 'V' | return 'line' | endif
  return 'block'
endfunction

function! s:State(m) abort
  let m = a:m[0]
  if m ==# 'n' | return 'normal' | endif
  if m ==# 'i' | return 'insert' | endif
  if m ==# 'R' | return 'replace' | endif
  if m ==# 'v' || m ==# 'V' || m ==# "\<C-v>" | return 'visual' | endif
  return a:m
endfunction

function! s:Reg(name) abort
  let text = getreg(a:name)
  if text ==# '' && getregtype(a:name) ==# ''
    return v:null
  endif
  return {'text': text, 'type': s:RegType(getregtype(a:name))}
endfunction

function! CompareCapture() abort
  " Evil's column counts characters, and it counts a combining mark as one.
  let col = strchars(strpart(getline('.'), 0, col('.') - 1))
  let s:res = {
        \ 'text': join(getline(1, '$'), "\n") . "\n",
        \ 'cursor': {'line': line('.') - 1, 'col': col},
        \ 'state': s:State(mode(1)),
        \ 'registers': {'"': s:Reg('"')}}
  for r in keys(s:want)
    if r =~# '^[a-z0]$'
      let s:res.registers[r] = s:Reg(r)
    endif
  endfor
endfunction

function! s:RunCase(c) abort
  enew!
  setlocal buftype=nofile noswapfile
  let w = get(a:c.input, 'window', {})
  if !empty(w)
    let &columns = w.width
    let &lines = w.height + 1
  endif
  " Buffer text: Vim keeps the final newline outside the lines.
  let text = a:c.input.text
  let lines = split(text, "\n", 1)
  if len(lines) > 1 && lines[-1] ==# ''
    call remove(lines, -1)
  endif
  call setline(1, lines)
  " Fresh state: undo history, registers, search, f/t.
  let ul = &undolevels
  set undolevels=-1
  exe "normal! a \<BS>\<Esc>"
  let &undolevels = ul
  for r in split('abcdefghijklmnopqrstuvwxyz0123456789-"', '\zs')
    call setreg(r, [])
  endfor
  let @/ = ''
  call setcharsearch({'char': ''})
  for [name, spec] in items(get(a:c.input, 'registers', {}))
    if spec.type ==# 'macro'
      call setreg(name, s:Notation(spec.text), 'v')
    else
      call setreg(name, spec.text, spec.type ==# 'line' ? 'V' : 'v')
    endif
  endfor
  if has_key(w, 'top_line')
    exe 'normal! ' . (w.top_line + 1) . 'Gzt'
  endif
  call setcursorcharpos(a:c.input.cursor.line + 1, a:c.input.cursor.col + 1)
  let prefix = ''
  if has_key(a:c.input, 'visual')
    let v = a:c.input.visual
    let mode = v.type ==# 'char' ? 'v' : v.type ==# 'line' ? 'V' : "\<C-v>"
    exe 'normal! ' . mode . "\<Esc>"
    call setcharpos("'<", [0, v.start.line + 1, v.start.col + 1, 0])
    " The selection's end is exclusive; '> is the last selected char.
    if v.end.col > 0
      call setcharpos("'>", [0, v.end.line + 1, v.end.col, 0])
    else
      call setcharpos("'>", [0, v.end.line, max([1, strchars(getline(v.end.line))]), 0])
    endif
    let prefix = 'gv'
    if a:c.input.cursor.byte == v.start.byte
      let prefix .= 'o'
    endif
  endif
  " The result is captured by a <Cmd> key after the case's keys, so it
  " sees the state the keys left (insert, visual...) without leaving it.
  let v:errmsg = ''
  messages clear
  let s:res = {}
  let s:want = a:c.expected.registers
  try
    call feedkeys(prefix . s:Keys(a:c.key_tokens) . "\<Cmd>call CompareCapture()\<CR>", 'tx')
  catch
    let v:errmsg = v:exception
  endtry
  if empty(s:res)
    " Vim dropped the rest of the keys (an error flushed them).
    call CompareCapture()
    let s:res.flushed = v:true
  endif
  let res = s:res
  let res.error = v:errmsg
  let res.beeps = len(filter(split(execute('messages'), "\n"), 'v:val =~# "Beep!"'))
  if mode() !=# 'n'
    call feedkeys("\<Esc>\<Esc>", 'tx')
  endif
  return res
endfunction

" Fields where Vim's result R differs from Evil's expected E.
function! s:Diff(input, e, r) abort
  let d = []
  " Vim always ends the last line with a newline, so when the case's text
  " has none, Vim's can't say how many newlines end Evil's.
  if a:input =~# '\n$'
    let etext = a:e.text =~# '\n$' ? a:e.text : a:e.text . "\n"
    let vtext = a:r.text
  else
    let etext = substitute(a:e.text, '\n\+$', '', '')
    let vtext = substitute(a:r.text, '\n\+$', '', '')
  endif
  if etext !=# vtext | call add(d, 'text') | endif
  if a:e.cursor.line != a:r.cursor.line || a:e.cursor.col != a:r.cursor.col
    call add(d, 'cursor')
  endif
  if a:e.state !=# a:r.state | call add(d, 'state') | endif
  let ee = get(a:e, 'errors', v:null)
  let ne = type(ee) == v:t_list ? len(ee) : 0
  let nv = a:r.beeps + (a:r.error !=# '' ? 1 : 0)
  if ne != nv | call add(d, 'errors ' . ne . ' vs ' . nv) | endif
  for [name, val] in items(a:r.registers)
    let ev = get(a:e.registers, name, v:null)
    if type(ev) == v:t_dict && ev.type ==# 'macro'
      let ev = {'text': ev.text, 'type': 'char'}
      if type(val) == v:t_dict
        let val = {'text': keytrans(val.text), 'type': val.type}
      endif
    endif
    if ev != val | call add(d, 'register ' . name) | endif
  endfor
  return d
endfunction

function! CompareFixture(path, out) abort
  let fx = json_decode(join(readfile(a:path), "\n"))
  let rows = []
  for c in fx.cases
    if fx.run ==# 'tty' && fx.topic ==# 'wrapped'
      " Vim wraps at the full width and Emacs's tty one column earlier,
      " so wrapped-line results aren't comparable here.
      call add(rows, json_encode({'topic': fx.topic, 'id': c.id, 'skipped': 'wrapping'}))
      continue
    endif
    if has_key(c.input, 'clipboard') || c.keys =~# '"[+*]'
      " This harness has no clipboard to compare with.
      call add(rows, json_encode({'topic': fx.topic, 'id': c.id, 'skipped': 'clipboard'}))
      continue
    endif
    let r = s:RunCase(c)
    call add(rows, json_encode({'topic': fx.topic, 'id': c.id,
          \ 'differs': s:Diff(c.input.text, c.expected, r), 'vim': r}))
  endfor
  call writefile(rows, a:out)
endfunction
