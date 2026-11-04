;; For each sample character, does forward-word from "a" stop before it
;; ("aXXX" in fundamental-mode)?  And the reverse ("XXXa").
(dolist (c '(?а ?α ?ա ?א ?ب ?ก ?क ?ა ?ᚠ ?Ա ?ه ?ж ?ω ?ä ?ß ?ŝ ?ɐ ?ḁ ?日 ?で ?テ ?한 ?ᄀ))
  (with-temp-buffer
    (fundamental-mode)
    (insert "a" (make-string 3 c) " z")
    (goto-char 1)
    (forward-word)
    (let ((fwd (1- (point))))
      (erase-buffer)
      (insert (make-string 3 c) "a z")
      (goto-char 1)
      (forward-word)
      (princ (format "%c U+%04X script=%s syntax=%c a->c-boundary=%s c->a-boundary=%s\n"
                     c c (aref char-script-table c) (char-syntax c)
                     (= fwd 1) (= (1- (point)) 3))))))
