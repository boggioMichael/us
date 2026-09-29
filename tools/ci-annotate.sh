#!/usr/bin/env bash
# Turns the errors in a build or test log into GitHub annotations, so a
# failed CI run says why on its page (and through the API) without anyone
# opening the full log.
#
#   some-command 2>&1 | tee out.log || { tools/ci-annotate.sh out.log "what failed"; exit 1; }
log="$1"
title="${2:-error}"
awk -v title="$title" '
  function flush() {
    if (buf != "") {
      gsub(/%/, "%25", buf); gsub(/\r/, "", buf); gsub(/\n/, "%0A", buf)
      print "::error title=" title "::" buf
      n++
      buf = ""
    }
  }
  /^error(\[|:)/ || /^warning: unused/ || /panicked at/ || /^---- .* stdout ----/ || /^test result: FAILED/ {
    flush()
    if (n < 10) { buf = $0; left = 16 }
    next
  }
  left > 0 { buf = buf "\n" $0; left--; if (left == 0) flush(); next }
  END { flush() }
' "$log"
