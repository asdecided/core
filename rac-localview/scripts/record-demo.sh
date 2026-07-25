#!/usr/bin/env bash
# Lore 90-second demo — every command below actually runs; nothing is mocked.
export PATH=/tmp/demo-venv/bin:$PATH
cd /home/user/requirements-as-code

type_cmd() {
  printf '\033[1;33m$\033[0m '
  local s="$1"
  for ((i = 0; i < ${#s}; i++)); do
    printf '%s' "${s:i:1}"
    sleep 0.025
  done
  sleep 0.4
  printf '\n'
}

run() {
  type_cmd "$1"
  eval "$1"
  sleep 1.2
}

sleep 0.5
run 'brew install asdecided/tap/asdecided-core'
run 'claude mcp add lore -- rac mcp'
run 'rac find "test topology" rac/'
run 'rac resolve ADR-027 rac/'
run 'decided validate decisions/'
sleep 1.5

# --- How this recording was made (reproducibility) -------------------------
# 1. python3 -m venv /tmp/demo-venv   (fresh venv; the script installs
#    requirements-as-code from PyPI inside the recording)
# 2. asciinema rec --overwrite --idle-time-limit 2 --cols 80 --rows 24 \
#      -c "bash scripts/record-demo.sh" design/demo.cast
# 3. npx svg-term-cli --in design/demo.cast --out src/landing/assets/demo.svg \
#      --window --no-cursor
# 4. Theme remap (sed) from svg-term defaults to the token palette:
#    #282d35->#1a1a18  #dbab79->#f5a623  #b9c0cb->#d6d4cc
#    #6f7783->#8f8d84  #a8cc8c->#57c97a  #e88388->#ce7878
# Every command in the cast actually ran; nothing is mocked.
