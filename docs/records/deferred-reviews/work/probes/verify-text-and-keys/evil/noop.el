(setq load-prefer-newer t native-comp-jit-compilation nil inhibit-automatic-native-compilation t native-comp-enable-subr-trampolines nil)
(add-to-list 'load-path (expand-file-name "~/.emacs.d/lisp/evil"))
(setq evil-want-integration t evil-want-keybinding nil)
(require 'evil)
(when (fboundp 'undo-redo) (evil-set-undo-system 'undo-redo))
(evil-mode 1)
(defun run-case (text pos keys)
  (with-current-buffer (get-buffer-create "s")
    (erase-buffer) (fundamental-mode) (evil-local-mode 1)
    (insert text) (buffer-enable-undo) (setq buffer-undo-list nil)
    (goto-char (1+ pos)) (evil-normal-state)
    (switch-to-buffer (current-buffer))
    (condition-case err (execute-kbd-macro keys)
      (error (message "  error %S" err)))
    (message "%S at %d, keys %S -> %S point %d" text pos keys (buffer-string) (1- (point)))))
;; x, then a no-op, then u: does u undo the x?
(run-case "abc a1 FOO" 0 "x3lrau")    ; r a on a
(run-case "abc a1 FOO" 0 "x4l~u")     ; ~ on the digit 1
(run-case "abc a1 FOO" 0 "x$gUiwu")   ; gU on FOO
(run-case "abc a1 FOO" 0 "x3lr<escape>u")
(run-case "abc a1 FOO" 0 "x$guiwgUiwu")
(run-case "abc a1 FOO" 0 "xd0u")      ; d0 at col 0
(run-case "abc a1 FOO" 0 "xi\eu")      ; empty insert session
(run-case "abc a1 FOO" 0 "x3lsa\eu")   ; s a on a
