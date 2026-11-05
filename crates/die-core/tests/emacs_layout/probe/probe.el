;;; probe.el --- where tty Emacs's visual-line-mode breaks rows  -*- lexical-binding: t -*-
;; Batch parent: emacs -Q --batch -l probe.el -f probe-main
;; It starts a child `emacs -Q -nw' on a pty (as tools/evil-oracle does), which
;; lays out each case and writes JSON to $PROBE_OUT.
(setq load-prefer-newer t native-comp-jit-compilation nil
      inhibit-automatic-native-compilation t native-comp-enable-subr-trampolines nil)
(require 'cl-lib)
(require 'json)

(defvar probe-dir (file-name-directory (or load-file-name buffer-file-name)))

(defun probe-size (w h)
  (set-frame-size (selected-frame) w (+ h 2))
  (redisplay t)
  (let ((tries 0))
    (while (and (< tries 5)
                (not (and (= (window-body-width) w) (= (window-body-height) h))))
      (set-frame-size (selected-frame)
                      (+ (frame-width) (- w (window-body-width)))
                      (+ (frame-height) (- h (window-body-height))))
      (redisplay t)
      (cl-incf tries))))

(defun probe-case (c)
  (let* ((text (alist-get 'text c))
         (w (alist-get 'tty_width c))
         (h 30)
         (tab (or (alist-get 'tab c) 8))
         (lnum (alist-get 'lnum c))
         (buf (generate-new-buffer "p")))
    (probe-size w h)
    (set-window-buffer (selected-window) buf)
    (set-buffer buf)
    (fundamental-mode)
    (setq tab-width tab)
    (setq truncate-lines nil)
    (visual-line-mode 1)
    (when lnum
      (setq display-line-numbers (intern lnum))
      (setq display-line-numbers-width 3))
    (insert text)
    (dolist (pr (append (alist-get 'props c) nil))
      (let ((s (1+ (aref pr 0))) (e (1+ (aref pr 1))) (k (intern (aref pr 2))) (v (aref pr 3)))
        (put-text-property s e k (if (eq v t) t v))))
    (dolist (ov (append (alist-get 'overlays c) nil))
      (let ((o (make-overlay (1+ (aref ov 0)) (1+ (aref ov 0)))))
        (overlay-put o (intern (aref ov 1)) (aref ov 2))))
    (goto-char (point-min))
    (set-window-start (selected-window) (point-min))
    (redisplay t)
    (let (posns starts)
      (dotimes (i (1+ (buffer-size)))
        (let ((pp (posn-at-point (1+ i))))
          (push (if pp
                    (let ((cr (posn-col-row pp)))
                      (vector (car cr) (cdr cr) (car (posn-x-y pp))))
                  :null)
                posns)))
      (save-excursion
        (goto-char (point-min))
        (while (progn (push (1- (point)) starts)
                      (and (= (vertical-motion 1) 1) (not (eobp))))))
      (prog1
          `((id . ,(alist-get 'id c))
            (body_width . ,(window-body-width))
            (starts . ,(vconcat (nreverse starts)))
            (posns . ,(vconcat (nreverse posns))))
        (kill-buffer buf)))))

(defun probe-child ()
  (let* ((cases (json-read-file (getenv "PROBE_CASES")))
         (out (cl-loop for c across cases collect (probe-case c))))
    (with-temp-file (getenv "PROBE_OUT")
      (insert (json-encode (vconcat out))))
    (kill-emacs 0)))

(defun probe-main ()
  (let* ((emacs (expand-file-name invocation-name invocation-directory))
         (process-environment (cons "TERM=vt100" process-environment))
         (proc (make-process
                :name "probe" :buffer nil :noquery t :connection-type 'pty
                :sentinel #'ignore :filter #'ignore
                :command `("sh" "-c" "stty rows 40 cols 150 && exec \"$@\"" "sh"
                           ,emacs "-Q" "-nw" "-l" ,(expand-file-name "probe.el" probe-dir)
                           "-f" "probe-child")))
         (deadline (+ (float-time) 60)))
    (while (and (process-live-p proc) (< (float-time) deadline))
      (accept-process-output proc 0.05))
    (when (process-live-p proc) (delete-process proc) (error "timed out"))
    (message "child exit %s" (process-exit-status proc))))
