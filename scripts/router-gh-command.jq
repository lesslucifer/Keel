# The owner's GitHub command grammar (E-GHFLOW.W3, the epic plan's open point 6): a comment's first
# line, matched against a closed list. scripts/router-gh-watch.sh includes it (jq -L scripts/,
# include "router-gh-command"); tests/suite/router_gh_watch_args.rs tests it alone.
#   /keel go | /keel hold | /keel retry | /keel answer <n> <A|B>       n: 1 to 999, no leading zero
# The keyword, the verb and the choice in any case (a phone capitalises); words split on spaces and
# tabs; a CRLF body reads as LF. A first line starting with /keel is a command or a bad command, never
# free text. W6 and W8 add their verbs here.
# keel_command: a comment body -> one of
#   {kind: "command", verb, n?, choice?, rest}   rest: the lines after the first
#   {kind: "bad", why, rest}                     why: not-keel, no-verb, unknown-verb, extra-words,
#                                                missing-words, bad-number, bad-choice
#   {kind: "text", rest}                         no command: every line is the message

def keel_lines: gsub("\r\n"; "\n") | if . == "" then [""] else split("\n") end;
def keel_trim: sub("^[ \t]+"; "") | sub("[ \t]+$"; "");
def keel_command:
  keel_lines as $l
  | ($l[0] | keel_trim) as $f
  | if ($f | ascii_downcase | startswith("/keel") | not) then {kind: "text", rest: $l}
    else
      [$f | splits("[ \t]+")] as $w
      | $l[1:] as $rest
      | (($w[1] // "") | ascii_downcase) as $v
      | if ($w[0] | ascii_downcase) != "/keel" then {kind: "bad", why: "not-keel", rest: $rest}
        elif $v == "" then {kind: "bad", why: "no-verb", rest: $rest}
        elif ($v | IN("go", "hold", "retry")) then
          if ($w | length) == 2 then {kind: "command", verb: $v, rest: $rest}
          else {kind: "bad", why: "extra-words", rest: $rest} end
        elif $v == "answer" then
          if ($w | length) < 4 then {kind: "bad", why: "missing-words", rest: $rest}
          elif ($w | length) > 4 then {kind: "bad", why: "extra-words", rest: $rest}
          elif ($w[2] | test("^[1-9][0-9]{0,2}$") | not) then {kind: "bad", why: "bad-number", rest: $rest}
          elif ($w[3] | ascii_upcase | IN("A", "B") | not) then {kind: "bad", why: "bad-choice", rest: $rest}
          else {kind: "command", verb: "answer", n: ($w[2] | tonumber), choice: ($w[3] | ascii_upcase), rest: $rest} end
        else {kind: "bad", why: "unknown-verb", rest: $rest} end
    end;
