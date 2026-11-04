" drive.vim: run cases from a JSON list in Vim and write one JSON line each.
"   vim --not-a-term -u NONE -i NONE -N -n -S drive.vim -c 'call Drive("in.json","out.jsonl")' -c 'qa!'
" Each case: {name, text (string), line, col (0-based), keys (Vim notation), opts (ex cmds), registers {name: [text, type]}, window {width,height,top_line}}
set nocompatible
set shiftwidth=4 softtabstop=0 expandtab autoindent
set ignorecase smartcase
set backspace=indent,eol,start
set clipboard=
set nomore noswapfile
set laststatus=0 noruler noshowcmd
set debug=beep

function! s:Notation(text) abort
  return substitute(a:text, '<\([A-Za-z][-A-Za-z]*\)>', '\=eval(''"\<'' . submatch(1) . ''>"'')', 'g')
endfunction

function! s:Reg(name) abort
  let text = getreg(a:name)
  if text ==# '' && getregtype(a:name) ==# '' | return v:null | endif
  let t = getregtype(a:name)
  return {'text': text, 'type': t ==# 'v' ? 'char' : t ==# 'V' ? 'line' : 'block'}
endfunction

function! Capture() abort
  let pos = getcursorcharpos()
  let s:res = {'text': join(getline(1, '$'), "\n") . "\n",
        \ 'line': pos[1] - 1, 'col': pos[2] - 1, 'mode': mode(1),
        \ 'topline': line('w0') - 1,
        \ 'regs': {}}
  for r in split('"0123456789-abc', '\zs')
    let v = s:Reg(r)
    if type(v) != v:t_none | let s:res.regs[r] = v | endif
  endfor
endfunction

function! s:Run(c) abort
  enew!
  setlocal buftype=nofile noswapfile
  set startofline& whichwrap& virtualedit& cpoptions& wrapscan& scrolloff=0
  set joinspaces&
  let w = get(a:c, 'window', {})
  if !empty(w)
    let &columns = w.width
    let &lines = w.height + 1
  else
    let &columns = 80
    let &lines = 25
  endif
  for o in get(a:c, 'opts', [])
    exe o
  endfor
  let lines = split(a:c.text, "\n", 1)
  if len(lines) > 1 && lines[-1] ==# '' | call remove(lines, -1) | endif
  call setline(1, lines)
  let ul = &undolevels
  set undolevels=-1
  exe "normal! a \<BS>\<Esc>"
  let &undolevels = ul
  for r in split('abcdefghijklmnopqrstuvwxyz0123456789-"', '\zs')
    call setreg(r, [])
  endfor
  let @/ = ''
  call setcharsearch({'char': ''})
  for [name, spec] in items(get(a:c, 'registers', {}))
    if spec[1] ==# 'macro'
      call setreg(name, s:Notation(spec[0]), 'v')
    else
      call setreg(name, spec[0], spec[1] ==# 'line' ? 'V' : 'v')
    endif
  endfor
  if has_key(w, 'top_line')
    exe 'normal! ' . (w.top_line + 1) . 'Gzt'
  endif
  call setcursorcharpos(a:c.line + 1, a:c.col + 1)
  messages clear
  let v:errmsg = ''
  let s:res = {}
  try
    call feedkeys(s:Notation(a:c.keys) . "\<Cmd>call Capture()\<CR>", 'tx')
  catch
    let v:errmsg = v:exception
  endtry
  if empty(s:res)
    call Capture()
    let s:res.flushed = v:true
  endif
  let msgs = split(execute('messages'), "\n")
  let s:res.beeps = len(filter(copy(msgs), 'v:val =~# "Beep!"'))
  let s:res.errmsg = v:errmsg
  let s:res.name = a:c.name
  if mode() !=# 'n' | call feedkeys("\<Esc>\<Esc>", 'tx') | endif
  return s:res
endfunction

function! Drive(inp, out) abort
  let cases = json_decode(join(readfile(a:inp), "\n"))
  let rows = []
  for c in cases
    call add(rows, json_encode(s:Run(c)))
  endfor
  call writefile(rows, a:out)
endfunction
