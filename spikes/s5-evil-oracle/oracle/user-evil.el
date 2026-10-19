;;; user-evil.el --- Mirror of the user's Evil settings for the oracle  -*- lexical-binding: t -*-

;; Loaded by evil-oracle.el in `emacs -Q --batch'.  Every setting below
;; mirrors a line of ~/.emacs.d/init.el that changes how Evil edits text.
;; Evil itself is loaded read-only from EVIL_ORACLE_EVIL_DIR; nothing here
;; may write under ~/.emacs.d.

;;; Never compile, never write.

(setq load-prefer-newer t)              ; load .el sources, never stale .elc
(setq native-comp-jit-compilation nil)  ; no deferred native compilation
(setq inhibit-automatic-native-compilation t)
(setq native-comp-enable-subr-trampolines nil) ; advising subrs would compile
(setq make-backup-files nil)            ; init.el:363
(setq auto-save-default nil)            ; init.el:364
(setq auto-save-list-file-prefix nil)
(setq create-lockfiles nil)
(setq history-length 0)

;;; Load Evil the way init.el:86-99 does.

(let ((dir (getenv "EVIL_ORACLE_EVIL_DIR")))
  (unless (and dir (file-exists-p (expand-file-name "evil.el" dir)))
    (error "EVIL_ORACLE_EVIL_DIR must point at the Evil source directory"))
  (add-to-list 'load-path dir))

(setq evil-want-integration t)          ; init.el:93
(setq evil-want-keybinding nil)         ; init.el:94
(require 'evil)
;; init.el:97-98 runs `evil-collection-init'.  It only adds bindings for
;; specific major modes (dired, magit, ...), not for text editing, so the
;; oracle skips it.
(evil-mode 1)                           ; init.el:99

;;; Settings that change editing behaviour.

(evil-set-leader 'normal (kbd "\\"))    ; init.el:284, `\' is a prefix key
(evil-set-leader 'visual (kbd "\\"))    ; init.el:285
(setq-default indent-tabs-mode nil)     ; init.el:330
(global-visual-line-mode 1)             ; init.el:335
(setq line-move-visual t)               ; init.el:344
(setq search-invisible t)               ; init.el:369
(setq enable-recursive-minibuffers t)   ; init.el:358

;;; Frame chrome.  Only matters for the tty run, where a menu bar would
;;; take a line from the window.

(menu-bar-mode -1)                      ; init.el:307
(tool-bar-mode -1)                      ; init.el:309
(when (fboundp 'scroll-bar-mode) (scroll-bar-mode -1)) ; init.el:308

;;; GUI defaults that batch mode changes.

;; In batch mode `transient-mark-mode' starts off; the GUI has it on.
(transient-mark-mode 1)

(provide 'user-evil)
;;; user-evil.el ends here
