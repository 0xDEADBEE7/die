;;; evil-oracle.el --- Run Vim key sequences through Evil, emit JSON fixtures  -*- lexical-binding: t -*-

;; Usage (see run-oracle.sh, which sets the environment):
;;
;;   emacs -Q --batch -L oracle -l evil-oracle -f evil-oracle-main CASES...
;;
;; Batch Emacs runs every case file except those marked "run": "tty".
;; Those need a real redisplay (window geometry, wrapped lines), so batch
;; Emacs then starts a child `emacs -Q -nw' on a pty it creates, which
;; runs them the same way and redisplays after every command.
;;
;; Environment:
;;   EVIL_ORACLE_EVIL_DIR  Evil source directory, loaded read-only
;;   EVIL_ORACLE_OUT       output directory for fixtures (required)
;;   EVIL_ORACLE_ORDER     "forward" (default) or "reverse": run cases in
;;                         reverse order, to prove cases don't leak state.
;;                         Output order is the same either way.
;;   EVIL_ORACLE_RUN       "batch" or "tty": force every file into one run
;;   EVIL_ORACLE_EMACS     Emacs binary for the tty run
;;   EVIL_ORACLE_LOG       set by the batch run for the tty child
;;   EVIL_ORACLE_DEBUG     log each command in the tty run
;;
;; Each CASES file is JSON: {"topic": ..., "cases": [CASE...]}.  Each
;; fixture file is JSON with the same cases plus Evil's results.  The
;; schema is documented in docs/records/s5-evil-oracle.md and mirrored by
;; the serde structs in src/lib.rs.

;; No compilation of any kind, before anything else loads.  The tty
;; child also gets these on its command line, ahead of this file, since
;; an interactive Emacs starts native compilation as soon as it loads a
;; library that lacks a .eln file.
(setq load-prefer-newer t
      native-comp-jit-compilation nil
      inhibit-automatic-native-compilation t
      native-comp-enable-subr-trampolines nil)

(require 'cl-lib)
(require 'subr-x)

(setq user-emacs-directory
      (file-name-as-directory (expand-file-name ".emacs.d" (getenv "HOME"))))
(require 'user-evil)

(defconst evil-oracle-schema 1)

;;;; Keys: Vim notation <-> Emacs events

(defconst evil-oracle--named-keys
  '(("Esc" . escape) ("CR" . ?\r) ("Enter" . ?\r) ("Return" . ?\r)
    ("NL" . ?\n) ("BS" . ?\d) ("Tab" . ?\t) ("Space" . ?\s)
    ("lt" . ?<) ("Bslash" . ?\\) ("Bar" . ?|) ("Del" . delete)
    ("Up" . up) ("Down" . down) ("Left" . left) ("Right" . right)
    ("Home" . home) ("End" . end))
  "Vim key names the oracle understands, with their Emacs events.
Matching is case-insensitive.  `<Esc>' must be the `escape' symbol:
a raw ESC character in a keyboard macro becomes a Meta prefix.")

(defun evil-oracle--named-key (name)
  "Return (TOKEN . EVENT) for the Vim key NAME, or nil if unknown."
  (if (string-match "\\`[Cc]-\\(.\\)\\'" name)
      (let ((ch (downcase (string-to-char (match-string 1 name)))))
        (cons (format "<C-%c>" ch) (aref (kbd (format "C-%c" ch)) 0)))
    (let ((hit (assoc-string name evil-oracle--named-keys t)))
      (and hit (cons (format "<%s>" (car hit)) (cdr hit))))))

(defun evil-oracle--parse-keys (keys)
  "Parse the Vim-notation string KEYS into a list of (TOKEN . EVENT).
`<Name>' is a key name when Name is known, otherwise a literal `<'."
  (let ((i 0) (n (length keys)) out)
    (while (< i n)
      (let ((c (aref keys i)) tok)
        (when (eq c ?<)
          (let ((close (string-search ">" keys (1+ i))))
            (when close
              (setq tok (evil-oracle--named-key (substring keys (1+ i) close)))
              (when tok (setq i (1+ close))))))
        (unless tok
          (setq tok (cons (string c) c)
                i (1+ i)))
        (push tok out)))
    (nreverse out)))

(defun evil-oracle--event-token (ev)
  "Return the Vim-notation token for the Emacs event EV."
  (cond
   ((eq ev ?<) "<lt>")
   ((memq ev '(escape ?\e)) "<Esc>")
   ((memq ev '(return ?\r)) "<CR>")
   ((eq ev ?\n) "<NL>")
   ((memq ev '(backspace ?\d)) "<BS>")
   ((memq ev '(tab ?\t)) "<Tab>")
   ((and (integerp ev) (<= 1 ev 26)) (format "<C-%c>" (+ ev 96)))
   ((characterp ev) (string ev))
   ((symbolp ev)
    (let ((hit (rassq ev evil-oracle--named-keys)))
      (if hit (format "<%s>" (car hit)) (format "<%s>" ev))))
   (t (format "<%S>" ev))))

(defun evil-oracle--keys-string (keys)
  "Return the Vim-notation string for the event sequence KEYS."
  (mapconcat #'evil-oracle--event-token (listify-key-sequence keys) ""))

;;;; Simulated macOS clipboard

;; The user's GUI Emacs pushes every `kill-new' to the macOS pasteboard
;; through `interprogram-cut-function' and reads it back through
;; `interprogram-paste-function'.  The oracle replaces both with a fake
;; clipboard, so it never touches the real one and records every write.

(defvar evil-oracle--clipboard nil "Text on the simulated clipboard.")
(defvar evil-oracle--clipboard-seen nil "Clipboard text Emacs already has.")
(defvar evil-oracle--clipboard-writes nil "Texts written, newest first.")
(defvar evil-oracle--primary nil "Text of the simulated PRIMARY selection.")

(defun evil-oracle--cut (text)
  "Record TEXT as written to the clipboard (`interprogram-cut-function')."
  (let ((s (substring-no-properties text)))
    (push s evil-oracle--clipboard-writes)
    (setq evil-oracle--clipboard s
          evil-oracle--clipboard-seen s)))

(defun evil-oracle--paste ()
  "Return clipboard text that Emacs hasn't seen (`interprogram-paste-function')."
  (when (and evil-oracle--clipboard
             (not (equal evil-oracle--clipboard evil-oracle--clipboard-seen)))
    (setq evil-oracle--clipboard-seen evil-oracle--clipboard)
    evil-oracle--clipboard))

(defun evil-oracle--gui-set-selection (type data)
  "Replacement for `gui-set-selection' (registers `\"+' and `\"*')."
  (cond ((eq type 'CLIPBOARD) (evil-oracle--cut (or data "")))
        ((eq type 'PRIMARY)
         (setq evil-oracle--primary (and (stringp data)
                                         (substring-no-properties data))))))

(defun evil-oracle--gui-get-selection (&optional type _data-type)
  "Replacement for `gui-get-selection'."
  (pcase (or type 'PRIMARY)
    ('CLIPBOARD evil-oracle--clipboard)
    ('PRIMARY evil-oracle--primary)))

(setq interprogram-cut-function #'evil-oracle--cut
      interprogram-paste-function #'evil-oracle--paste)
(advice-add 'gui-set-selection :override #'evil-oracle--gui-set-selection)
(advice-add 'gui-get-selection :override #'evil-oracle--gui-get-selection)

;;;; Typed keys

;; A case's keys run as one keyboard macro.  Feeding them through
;; `unread-command-events' instead would starve the keyboard macros that
;; `.' and `@a' run, because Emacs reads unread events before macro
;; events.  Three pieces of advice make the macro behave like typing:
;;
;; - Errors.  Interactively, a command that signals an error doesn't
;;   stop the keys typed after it; inside a macro it ends the macro.  So
;;   errors from the case's own commands are recorded and the remaining
;;   keys still run, while errors inside a nested macro (`@a') end that
;;   macro, as they do in the GUI.
;;
;; - Recording.  `kmacro-start-macro' refuses to record while a macro
;;   runs, and Emacs never records events that come from a macro.  While
;;   `q' records, the oracle tracks which of the case's keys were typed
;;   and hands them to `end-kbd-macro' as the recording, exactly as the
;;   keyboard would have.
;;
;; - The bell.  In the GUI, a bell inside a running macro ends it; batch
;;   Emacs only prints BEL.

(defvar evil-oracle--top-macro nil "The case's key vector while it runs.")
(defvar evil-oracle--errors nil "Errors recorded for the case, newest first.")
(defvar evil-oracle--recording-start nil
  "Index into the case's keys where the current `q' recording starts.")

(defun evil-oracle--top-level-p ()
  "Non-nil while Emacs reads the case's own keys (not a nested macro)."
  (and evil-oracle--top-macro (eq executing-kbd-macro evil-oracle--top-macro)))

(defun evil-oracle--error-json (err index)
  "Return the JSON form of the error ERR, signalled after INDEX keys.
Long messages (some print whole keymaps) are cut to 160 characters."
  (let ((msg (error-message-string err)))
    `((symbol . ,(symbol-name (car err)))
      (message . ,(if (> (length msg) 160) (concat (substring msg 0 160) "...") msg))
      (after_keys . ,index))))

(defun evil-oracle--command-execute (orig &rest args)
  "Around advice for `command-execute' giving typed-key error semantics."
  (if (not (evil-oracle--top-level-p))
      (apply orig args)
    (condition-case err
        (apply orig args)
      (error
       (push (evil-oracle--error-json err executing-kbd-macro-index)
             evil-oracle--errors)
       ;; What the command loop's error handler does.
       (setq prefix-arg nil)
       nil))))

(defun evil-oracle--kmacro-start (orig &rest args)
  "Around advice for `kmacro-start-macro': allow `q' in the case's keys."
  (if (not (evil-oracle--top-level-p))
      (apply orig args)
    (let ((executing-kbd-macro nil))
      (apply orig args))
    (when defining-kbd-macro
      (setq evil-oracle--recording-start executing-kbd-macro-index))))

(defun evil-oracle--end-kbd-macro (orig &rest args)
  "Around advice for `end-kbd-macro': store the keys typed while recording.
Like the keyboard, the recording stops before the keys of the command
that ends it."
  (let ((start evil-oracle--recording-start))
    (setq evil-oracle--recording-start nil)
    (prog1 (apply orig args)
      (when (and start (evil-oracle--top-level-p))
        (setq last-kbd-macro
              (substring evil-oracle--top-macro start
                         (- executing-kbd-macro-index
                            (length (this-command-keys-vector)))))))))

(defun evil-oracle--ding (_orig &optional arg)
  "Around advice for `ding' (see above); the bell itself stays silent."
  (when (and executing-kbd-macro (not (evil-oracle--top-level-p)) (null arg))
    (user-error "Keyboard macro terminated by a command ringing the bell")))

(advice-add 'command-execute :around #'evil-oracle--command-execute)
(advice-add 'kmacro-start-macro :around #'evil-oracle--kmacro-start)
(advice-add 'end-kbd-macro :around #'evil-oracle--end-kbd-macro)
(advice-add 'ding :around #'evil-oracle--ding)

;; A message wider than a narrow tty frame would grow the echo area and
;; shrink the case's window mid-case.  Keep the echo area at one line.
(setq resize-mini-windows nil)

;;;; Global state reset

(defconst evil-oracle--global-prefixes
  '("evil-" "isearch-" "search-ring" "regexp-search-ring" "kill-ring"
    "query-replace-" "kmacro-")
  "Name prefixes of global variables restored before every case.")

(defvar evil-oracle--snapshot nil
  "Alist of (SYMBOL . VALUE) taken after init, restored before each case.")

(defun evil-oracle--snapshot-p (sym)
  (and (boundp sym)
       (not (keywordp sym))
       (let ((name (symbol-name sym)))
         (cl-some (lambda (p) (string-prefix-p p name))
                  evil-oracle--global-prefixes))
       (let ((v (default-value sym)))
         (not (or (keymapp v) (functionp v) (bufferp v) (processp v)
                  (and (consp v) (keymapp (car-safe v))))))
       ;; Mode variables are switched by their modes, and the oracle's
       ;; own variables must survive.
       (not (string-suffix-p "-mode" (symbol-name sym)))
       (not (string-prefix-p "evil-oracle-" (symbol-name sym)))))

(defun evil-oracle--take-snapshot ()
  (let (snap)
    (mapatoms (lambda (sym)
                (when (evil-oracle--snapshot-p sym)
                  (push (cons sym (copy-tree (default-value sym) t)) snap))))
    (setq evil-oracle--snapshot
          (sort snap (lambda (a b) (string< (car a) (car b)))))))

(defun evil-oracle--restore-globals ()
  (dolist (cell evil-oracle--snapshot)
    (ignore-errors (set-default (car cell) (copy-tree (cdr cell) t))))
  (setq kill-ring nil
        kill-ring-yank-pointer nil
        register-alist nil
        last-kbd-macro nil
        search-ring nil
        regexp-search-ring nil
        pending-undo-list nil
        last-command nil
        this-command nil
        real-last-command nil
        prefix-arg nil
        evil-oracle--clipboard nil
        evil-oracle--clipboard-seen nil
        evil-oracle--clipboard-writes nil
        evil-oracle--primary nil
        evil-oracle--errors nil
        evil-oracle--recording-start nil))

;;;; Case input

(defun evil-oracle--get (case key &optional default)
  (let ((cell (assq key case)))
    (if cell (cdr cell) default)))

(defun evil-oracle--join-text (text)
  "TEXT is a string (exact buffer text) or a list of lines (each ends in \\n)."
  (if (stringp text) text
    (mapconcat (lambda (l) (concat l "\n")) text "")))

(defun evil-oracle--parse-text (raw marker visual)
  "Split RAW into (TEXT CURSOR VBEG VEND), 0-based char offsets.
MARKER is the cursor marker character.  When VISUAL is non-nil,
`<' and `>' mark the start and the exclusive end of the selection."
  (let ((pos 0) chars cursor vbeg vend)
    (dolist (c (string-to-list raw))
      (cond
       ((eq c marker)
        (when cursor (error "More than one cursor marker"))
        (setq cursor pos))
       ((and visual (eq c ?<) (not vbeg)) (setq vbeg pos))
       ((and visual (eq c ?>) (not vend)) (setq vend pos))
       (t (push c chars) (cl-incf pos))))
    (unless cursor (error "No cursor marker"))
    (when (and visual (not (and vbeg vend)))
      (error "Visual case needs both < and > markers"))
    (list (concat (nreverse chars)) cursor vbeg vend)))

(defun evil-oracle--register-char (name)
  (unless (= (length name) 1) (error "Bad register name %S" name))
  (aref name 0))

(defun evil-oracle--register-spec (spec)
  "Return (TEXT . TYPE) for an input register SPEC (string or object)."
  (if (stringp spec) (cons spec "char")
    (cons (evil-oracle--get spec 'text) (evil-oracle--get spec 'type "char"))))

(defun evil-oracle--yank-into (reg text type)
  "Store TEXT of TYPE the way Evil does.
TYPE \"char\" or \"line\" stores it as a yank would; \"macro\" stores
TEXT, in Vim key notation, as a recorded keyboard macro.  REG nil means
the unnamed register (the kill ring)."
  (if (equal type "macro")
      (evil-set-register reg (vconcat (mapcar #'cdr (evil-oracle--parse-keys text))))
    (with-temp-buffer
      (insert text)
      (pcase type
        ("char" (evil-yank-characters (point-min) (point-max) reg))
        ("line" (evil-yank-lines (point-min) (point-max) reg))
        (_ (error "Initial register type must be char, line or macro, not %S"
                  type))))))

(defun evil-oracle--setup-registers (specs clipboard)
  "Set registers from SPECS, an alist of (NAME . SPEC), and the clipboard."
  (let (zero unnamed)
    (dolist (cell specs)
      (let ((name (symbol-name (car cell)))
            (spec (evil-oracle--register-spec (cdr cell))))
        (cond ((equal name "\"") (setq unnamed spec))
              ((equal name "0") (setq zero spec))
              (t (evil-oracle--yank-into (evil-oracle--register-char name)
                                         (car spec) (cdr spec))))))
    ;; Evil's yank helpers also fill the kill ring and "0; undo that.
    (setq kill-ring nil kill-ring-yank-pointer nil)
    (set-register ?0 nil)
    (when zero
      (evil-oracle--yank-into ?0 (car zero) (cdr zero))
      (setq kill-ring nil kill-ring-yank-pointer nil))
    (when unnamed
      (let ((saved-zero (get-register ?0)))
        (evil-oracle--yank-into nil (car unnamed) (cdr unnamed))
        (set-register ?0 saved-zero)))
    ;; The clipboard holds the unnamed text unless another app copied
    ;; something later (the case's "clipboard" field).
    (setq evil-oracle--clipboard-writes nil
          evil-oracle--clipboard-seen (and unnamed (car unnamed))
          evil-oracle--clipboard (or clipboard (and unnamed (car unnamed))))))

;;;; Results

(defun evil-oracle--pos (p)
  "Return the JSON position of buffer position P: 0-based line,
0-based column in characters, and 0-based UTF-8 byte offset."
  (save-excursion
    (goto-char p)
    `((line . ,(1- (line-number-at-pos p t)))
      (col . ,(- p (line-beginning-position)))
      (byte . ,(length (encode-coding-string
                        (buffer-substring-no-properties (point-min) p)
                        'utf-8-unix t))))))

(defun evil-oracle--register-type (text)
  (let ((handler (car-safe (get-text-property 0 'yank-handler text))))
    (cond ((eq handler 'evil-yank-line-handler) "line")
          ((eq handler 'evil-yank-block-handler) "block")
          (t "char"))))

(defun evil-oracle--register-json (value)
  "Return the JSON form of a register VALUE, or nil if it's empty."
  (cond
   ((null value) nil)
   ((vectorp value)
    `((text . ,(evil-oracle--keys-string value)) (type . "macro")))
   ((stringp value)
    `((text . ,(substring-no-properties value))
      (type . ,(evil-oracle--register-type value))))
   (t `((text . ,(format "%S" value)) (type . "other")))))

(defconst evil-oracle--register-names
  (append (string-to-list "0123456789") (number-sequence ?a ?z) '(?-)))

(defun evil-oracle--registers ()
  "Return an alist of register char -> JSON value for every register."
  (let ((interprogram-paste-function nil))
    (cl-loop for r in evil-oracle--register-names
             collect (cons r (evil-oracle--register-json
                              (if (eq r ?-) evil-last-small-deletion
                                (evil-get-register r t)))))))

(defun evil-oracle--state ()
  (symbol-name evil-state))

(defun evil-oracle--visual ()
  (when (evil-visual-state-p)
    `((type . ,(symbol-name evil-visual-selection))
      (start . ,(evil-oracle--pos evil-visual-beginning))
      (end . ,(evil-oracle--pos evil-visual-end)))))

(defun evil-oracle--layout ()
  "Visual line starts of the buffer in the selected window, as byte offsets."
  (save-excursion
    (goto-char (point-min))
    (let (starts)
      (while (progn (push (alist-get 'byte (evil-oracle--pos (point))) starts)
                    (and (= (vertical-motion 1) 1) (not (eobp)))))
      (vconcat (nreverse starts)))))

(defun evil-oracle--window-json ()
  `((width . ,(window-body-width))
    (height . ,(window-body-height))
    (top_line . ,(1- (line-number-at-pos (window-start) t)))))

;;;; Running a case

(defun evil-oracle--result (buf before window-spec)
  "Return the expected-result JSON for BUF after the keys ran.
BEFORE is the register alist from before the keys."
  (with-current-buffer buf
    (let* ((win (get-buffer-window buf))
           (pt (if win (window-point win) (point)))
           (after (evil-oracle--registers))
           (unnamed (car kill-ring-yank-pointer))
           (changed (cl-loop for (r . v) in after
                             unless (equal v (alist-get r before))
                             collect (cons (intern (string r)) (or v :null)))))
      `((text . ,(buffer-substring-no-properties (point-min) (point-max)))
        (cursor . ,(evil-oracle--pos pt))
        (state . ,(evil-oracle--state))
        ,@(when (evil-visual-state-p)
            `((visual . ,(evil-oracle--visual))))
        (registers
         . ((\" . ,(or (evil-oracle--register-json unnamed) :null))
            ,@changed))
        (clipboard
         . ((text . ,(or evil-oracle--clipboard :null))
            (writes . ,(vconcat (reverse evil-oracle--clipboard-writes)))))
        ,@(when window-spec
            `((window . ,(with-selected-window (or win (selected-window))
                           (evil-oracle--window-json)))))
        ,@(when evil-oracle--errors
            `((errors . ,(vconcat (reverse evil-oracle--errors)))))))))

(defun evil-oracle--configure-window (spec)
  "Size the frame so the window body is WIDTH x HEIGHT, from SPEC.
The mode line and the echo area take a line each.  Batch frames don't
report their new size and tty frames can be off by a line, so the tty
run corrects the size until the body fits.  The pty is larger than any
case's frame."
  (let ((w (evil-oracle--get spec 'width 80))
        (h (evil-oracle--get spec 'height 22))
        (tries 0))
    (message nil)
    (unless noninteractive (redisplay t))
    ;; Batch frames don't report their new size, so start from the
    ;; usual overhead and correct only in the tty run.
    (set-frame-size (selected-frame) w (+ h 2))
    (unless noninteractive (redisplay t))
    (while (and (not noninteractive) (< tries 3)
                (not (and (= (window-body-width) w) (= (window-body-height) h))))
      (set-frame-size (selected-frame)
                      (+ (frame-width) (- w (window-body-width)))
                      (+ (frame-height) (- h (window-body-height))))
      (unless noninteractive (redisplay t))
      (cl-incf tries))
    (unless (and (= (window-body-width) w) (= (window-body-height) h))
      (error "Window is %dx%d (frame %dx%d), the case wants %dx%d"
             (window-body-width) (window-body-height)
             (frame-width) (frame-height) w h))))

(defun evil-oracle--redisplay ()
  "In the tty run, redisplay after each typed command, as the GUI does
between keystrokes.  Window start, `H', `L' and `gj' depend on it.
`redisplay' does nothing while a keyboard macro runs, so the case's
macro is hidden from it for the moment."
  (when (and (not noninteractive) (evil-oracle--top-level-p))
    (let ((executing-kbd-macro nil))
      (redisplay t))
    (when (getenv "EVIL_ORACLE_DEBUG")
      (evil-oracle--log "DEBUG cmd %S keys %S pt %d line %d ws-line %d frame %dx%d body %d mini %d"
                        this-command (this-command-keys-vector) (point)
                        (line-number-at-pos) (line-number-at-pos (window-start))
                        (frame-width) (frame-height) (window-body-height)
                        (window-total-height (minibuffer-window))))))

(defun evil-oracle--run-case (case)
  "Run CASE (an alist) and return its JSON alist."
  (evil-oracle--restore-globals)
  (let* ((id (evil-oracle--get case 'id))
         (keys (evil-oracle--get case 'keys))
         (toks (evil-oracle--parse-keys keys))
         (events (vconcat (mapcar #'cdr toks)))
         (marker (string-to-char (evil-oracle--get case 'cursor_marker "|")))
         (visual (evil-oracle--get case 'visual))
         (parsed (evil-oracle--parse-text
                  (evil-oracle--join-text (evil-oracle--get case 'text))
                  marker visual))
         (window-spec (evil-oracle--get case 'window))
         (mode (intern (evil-oracle--get case 'mode "fundamental-mode")))
         (timers-before (append timer-list timer-idle-list))
         (buf (generate-new-buffer "evil-oracle-case"))
         input expected)
    (unless (fboundp mode) (error "%s: unknown mode %s" id mode))
    ;; A case can split the frame (`:vsplit'); the next starts on one window.
    (delete-other-windows)
    (evil-oracle--configure-window window-spec)
    (evil-oracle--setup-registers (evil-oracle--get case 'registers)
                                  (evil-oracle--get case 'clipboard))
    (unwind-protect
        (progn
          (set-window-buffer (selected-window) buf)
          (set-buffer buf)
          (funcall mode)
          (evil-local-mode 1)
          (when global-visual-line-mode (visual-line-mode 1))
          ;; Redisplay last, after Evil's own post-command work.
          (add-hook 'post-command-hook #'evil-oracle--redisplay 100 t)
          (insert (nth 0 parsed))
          (buffer-enable-undo)
          (setq buffer-undo-list nil)
          (set-buffer-modified-p nil)
          (goto-char (1+ (nth 1 parsed)))
          (evil-normal-state)
          (set-window-start (selected-window)
                            (save-excursion
                              (goto-char (point-min))
                              (forward-line (evil-oracle--get window-spec 'top_line 0))
                              (point)))
          (set-window-point (selected-window) (point))
          (when visual
            (let* ((beg (1+ (nth 2 parsed)))
                   (end (1+ (nth 3 parsed)))
                   (pt (point))
                   (mark (cond ((= pt beg) (1- end))
                               ((= pt (1- end)) beg)
                               (t (error "%s: cursor must be at either end of the selection" id)))))
              (evil-visual-make-selection mark pt (intern visual))
              (evil-visual-refresh)))
          (unless noninteractive (redisplay t))
          (setq input
                `((text . ,(buffer-substring-no-properties (point-min) (point-max)))
                  (cursor . ,(evil-oracle--pos (point)))
                  ,@(when visual `((visual . ,(evil-oracle--visual))))
                  ,@(when-let* ((specs (evil-oracle--get case 'registers)))
                      `((registers
                         . ,(cl-loop for (name . spec) in specs
                                     for s = (evil-oracle--register-spec spec)
                                     collect (cons name `((text . ,(car s))
                                                          (type . ,(cdr s))))))))
                  ,@(when (evil-oracle--get case 'clipboard)
                      `((clipboard . ,(evil-oracle--get case 'clipboard))))
                  ,@(when window-spec
                      `((window . ,(evil-oracle--window-json))
                        (layout . ,(evil-oracle--layout))))))
          (let ((before (evil-oracle--registers)))
            (setq evil-oracle--top-macro events)
            (unwind-protect
                (condition-case err
                    (execute-kbd-macro events)
                  ;; Errors outside any command, e.g. while reading keys.
                  (error (push (evil-oracle--error-json err (length events))
                               evil-oracle--errors)))
              (setq evil-oracle--top-macro nil))
            (setq expected (evil-oracle--result buf before window-spec))))
      ;; Leave nothing running for the next case.
      (when (bound-and-true-p isearch-mode) (ignore-errors (isearch-done)))
      (setq overriding-terminal-local-map nil)
      (when defining-kbd-macro (ignore-errors (end-kbd-macro)))
      (with-current-buffer buf
        (when (evil-visual-state-p) (ignore-errors (evil-exit-visual-state))))
      (dolist (tm (append timer-list timer-idle-list))
        (unless (memq tm timers-before) (cancel-timer tm)))
      (set-window-buffer (selected-window) (get-buffer-create "*scratch*"))
      (set-buffer "*scratch*")
      (kill-buffer buf))
    `((id . ,id)
      ,@(when-let* ((d (evil-oracle--get case 'description))) `((description . ,d)))
      (source . "oracle")
      (keys . ,keys)
      (key_tokens . ,(vconcat (mapcar #'car toks)))
      (mode . ,(symbol-name mode))
      (input . ,input)
      (expected . ,expected)
      ,@(when-let* ((v (evil-oracle--get case 'vim))) `((vim . ,v))))))

;;;; Files

(defun evil-oracle--read-json-file (file)
  (with-temp-buffer
    (let ((coding-system-for-read 'utf-8))
      (insert-file-contents file))
    (json-parse-buffer :object-type 'alist :array-type 'list
                       :null-object nil :false-object nil)))

(defun evil-oracle--evil-source-hash ()
  "SHA-256 over Evil's .el files, naming the exact Evil the fixtures came from.
The user's Evil is vendored inside ~/.emacs.d, so it has no commit of its own."
  (let ((dir (getenv "EVIL_ORACLE_EVIL_DIR")))
    (secure-hash
     'sha256
     (mapconcat (lambda (f)
                  (with-temp-buffer
                    (set-buffer-multibyte nil)
                    (insert-file-contents-literally f)
                    (format "%s %s\n" (file-name-nondirectory f)
                            (secure-hash 'sha256 (current-buffer)))))
                (directory-files dir t "\\`evil.*\\.el\\'")
                ""))))

(defun evil-oracle--header (topic run)
  `((schema . ,evil-oracle-schema)
    (topic . ,topic)
    (generator . "evil-oracle")
    (run . ,run)
    (evil_version . ,evil-version)
    (evil_source_sha256 . ,(evil-oracle--evil-source-hash))
    (emacs_version . ,emacs-version)))

(defun evil-oracle--write-fixture (file topic run results)
  "Write RESULTS for TOPIC to FILE: a header, then one case per line."
  (let ((header (json-serialize (evil-oracle--header topic run))))
    (with-temp-buffer
      (insert (substring header 0 -1) ",\"cases\":[\n")
      (insert (mapconcat #'json-serialize results ",\n"))
      (insert "\n]}\n")
      (let ((coding-system-for-write 'utf-8-unix))
        (write-region (point-min) (point-max) file nil 'silent)))))

(defun evil-oracle--log (fmt &rest args)
  "Log to EVIL_ORACLE_LOG if set (the tty run has no stderr), else stderr."
  (let ((line (apply #'format fmt args))
        (log (getenv "EVIL_ORACLE_LOG")))
    (if (not log)
        (message "%s" line)
      (let ((coding-system-for-write 'utf-8-unix))
        (write-region (concat line "\n") nil log t 'silent)))))

(defun evil-oracle--file-run (json)
  "Which run a case file belongs to: its \"run\" field, default batch.
EVIL_ORACLE_RUN=tty or =batch forces every file into one run, which is
how the two runs are checked against each other."
  (or (getenv "EVIL_ORACLE_RUN") (evil-oracle--get json 'run "batch")))

(defun evil-oracle--run-files (files)
  "Run the case files in FILES that belong to this run; write fixtures."
  (let* ((out (or (getenv "EVIL_ORACLE_OUT") (error "EVIL_ORACLE_OUT not set")))
         (reverse (equal (getenv "EVIL_ORACLE_ORDER") "reverse"))
         (run (if noninteractive "batch" "tty"))
         (t0 (float-time))
         (count 0)
         jobs)
    (make-directory out t)
    (evil-oracle--take-snapshot)
    ;; Read everything first: (FILE TOPIC CASES).  A case file with
    ;; "run": "tty" belongs to the tty run, every other file to batch.
    (dolist (file files)
      (let* ((json (evil-oracle--read-json-file file))
             (topic (evil-oracle--get json 'topic))
             ;; File-level "defaults" fill in fields a case leaves out.
             (defaults (evil-oracle--get json 'defaults))
             (cases (mapcar (lambda (c)
                              (append c (cl-remove-if (lambda (d) (assq (car d) c))
                                                      defaults)))
                            (evil-oracle--get json 'cases))))
        (unless (equal topic (file-name-base file))
          (error "%s: topic %S must match the file name" file topic))
        (when (equal (evil-oracle--file-run json) run)
          (push (list file topic cases) jobs))))
    (setq jobs (nreverse jobs))
    ;; Run, in forward or reverse order, keeping results by position.
    (let* ((flat (cl-loop for (_ topic cases) in jobs
                          append (cl-loop for c in cases collect (cons topic c))))
           (order (if reverse (reverse flat) flat))
           (results (make-hash-table :test 'eq)))
      (dolist (item order)
        (let ((case (cdr item)))
          (puthash item
                   (condition-case err
                       (evil-oracle--run-case case)
                     (error (error "Case %s/%s: %s" (car item)
                                   (evil-oracle--get case 'id)
                                   (error-message-string err))))
                   results)
          (cl-incf count)))
      (cl-loop for (_ topic _cases) in jobs
               for items = (cl-remove-if-not (lambda (it) (eq (car it) topic)) flat)
               do (evil-oracle--write-fixture
                   (expand-file-name (concat topic ".json") out) topic run
                   (mapcar (lambda (it) (gethash it results)) items))))
    (evil-oracle--log "evil-oracle: %s run, %d cases in %.2fs (%s order)"
                      run count (- (float-time) t0)
                      (if reverse "reverse" "forward"))))

(defconst evil-oracle--no-compile
  "(setq native-comp-jit-compilation nil inhibit-automatic-native-compilation t native-comp-enable-subr-trampolines nil load-prefer-newer t)"
  "Evaluated first in the tty child, so it never compiles anything.")

(defconst evil-oracle--tty-size '(40 . 120)
  "Rows and columns of the pty the tty run gets; cases size frames within it.")

(defun evil-oracle--run-tty (files)
  "Run FILES' tty cases in a child `emacs -nw' on a pty, and wait for it.
The pty is created by Emacs itself and never written to, so the child
sees no stray input.  Returns the child's exit status."
  (let* ((emacs (or (getenv "EVIL_ORACLE_EMACS")
                    (expand-file-name invocation-name invocation-directory)))
         (log (make-temp-file "evil-oracle-tty" nil ".log"))
         ;; vt100, because xterm's terminal setup queries the terminal
         ;; and waits 2 s for an answer nobody sends.
         (process-environment
          (append (list "TERM=vt100" (concat "EVIL_ORACLE_LOG=" log))
                  process-environment))
         (dir (file-name-directory (locate-library "evil-oracle")))
         (proc (make-process
                :name "evil-oracle-tty" :buffer nil :noquery t
                :connection-type 'pty :sentinel #'ignore
                :filter (lambda (_p s) (when (getenv "EVIL_ORACLE_DEBUG")
                                         (message "tty: %S" s)))
                :command `("sh" "-c" "stty rows \"$1\" cols \"$2\" && shift 2 && exec \"$@\""
                           "sh" ,(number-to-string (car evil-oracle--tty-size))
                           ,(number-to-string (cdr evil-oracle--tty-size))
                           ,emacs "-Q" "-nw" "--eval" ,evil-oracle--no-compile
                           "-L" ,dir "-l" "evil-oracle"
                           "-f" "evil-oracle-main" ,@files)))
         (deadline (+ (float-time) 60)))
    (while (and (process-live-p proc) (< (float-time) deadline))
      (accept-process-output proc 0.05))
    (when (process-live-p proc)
      (delete-process proc)
      (evil-oracle--log "evil-oracle: error: tty run timed out"))
    (with-temp-buffer
      (insert-file-contents log)
      (when (> (buffer-size) 0)
        (message "%s" (string-trim-right (buffer-string)))))
    (delete-file log)
    (process-exit-status proc)))

(defun evil-oracle-main ()
  "Entry point: run the case files in `command-line-args-left'.
Batch Emacs runs the batch cases, then starts the tty run.  In the tty
run there's no stderr and Emacs doesn't exit by itself, so errors go to
the log and Emacs is killed with a status."
  (let ((files command-line-args-left))
    (setq command-line-args-left nil)
    (if noninteractive
        (progn
          (evil-oracle--run-files files)
          (when (cl-some (lambda (f) (equal (evil-oracle--file-run
                                             (evil-oracle--read-json-file f))
                                            "tty"))
                         files)
            (let ((status (evil-oracle--run-tty files)))
              (unless (eql status 0)
                (error "The tty run failed with status %s" status)))))
      (condition-case err
          (progn (evil-oracle--run-files files) (kill-emacs 0))
        (error (evil-oracle--log "evil-oracle: error: %s" (error-message-string err))
               (kill-emacs 1))))))

(provide 'evil-oracle)
;;; evil-oracle.el ends here
