#!/bin/bash
# Mutation check: break the core in one specific way at a time and confirm the property tests
# catch it. Sources are backed up under target/ and restored after each mutation.
# Usage: ./mutate.sh   (about 2 minutes; 2000 cases per property)
set -u
cd "$(dirname "$0")"
BK=target/mutate-backup
rm -rf "$BK" && mkdir -p "$BK" && cp src/*.rs "$BK/"
restore() { cp "$BK"/*.rs src/; }
trap restore EXIT

mutate() {
  local name="$1" file="$2" old="$3" new="$4"
  python3 - "$file" "$old" "$new" <<'EOF'
import sys
p, o, n = sys.argv[1:4]
s = open(p).read()
assert s.count(o) == 1, (p, o)
open(p, "w").write(s.replace(o, n))
EOF
  local out
  out=$(S4_CASES=2000 cargo test --test prop 2>&1)
  if printf '%s\n' "$out" 2>/dev/null | grep -q "test result: ok"; then
    echo "NOT CAUGHT  $name"
  else
    echo "caught      $name  <- $(printf '%s\n' "$out" 2>/dev/null | grep -oE "^test [a-z_]+ \.\.\. FAILED" | sed 's/ \.\.\. FAILED//; s/^test //' | tr '\n' ' ')"
    printf '%s\n' "$out" 2>/dev/null | grep -m1 -E "(step [0-9]+:|refused|wrong text|mismatch)" | sed 's/^/              /' | cut -c1-160
  fi
  restore
}

mutate "M1 layout cache key ignores decorations" src/layout.rs \
  "let key = (self.rev(), self.deco_gen, self.settings.tab_width);" \
  "let key = (self.rev(), 0, self.settings.tab_width);"
mutate "M2 hide merge ignores the replacement preference" src/layout.rs \
  "v.sort_by_key(|s| (s.start, s.repl.is_none(), std::cmp::Reverse(s.end), s.deco));" \
  "v.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end), s.deco));"
mutate "M3 no cursor fix-up after decorate" src/text.rs \
  "        self.deco_gen += 1;
        self.normalize_views();
        Ok(id)" \
  "        self.deco_gen += 1;
        Ok(id)"
mutate "M4 undo transform tie-break" src/undo.rs \
  "if p.pos + p.del <= o.pos {" \
  "if p.pos + p.del < o.pos {"
mutate "M5 end-of-line stop on a full row doesn't wrap" src/layout.rs \
  "    fn stop_here(&mut self, pos: usize) {
        if !self.fits(1) {
            self.newrow();
        }" \
  "    fn stop_here(&mut self, pos: usize) {"
mutate "M6 open undo group not rebased over silent edits" src/undo.rs \
  "            if !pending.is_empty() {" \
  "            if false {"
mutate "M7 undo drops the pending silent shifts" src/undo.rs \
  "        if !p.pending.is_empty() && !from.is_empty() {" \
  "        if false {"
mutate "M8 wide char ignores the row end (no filler)" src/layout.rs \
  "        if w > 1 && !self.fits(w) && self.col > 0 {" \
  "        if false {"
