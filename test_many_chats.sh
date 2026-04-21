#!/bin/bash

if [ $# -lt 1 ]; then
  echo "Usage: $0 <n>"
  exit 1
fi

N=$1
shift
CMD="cargo run --example chat"
SESSION="multi-$$"

tmux new-session -d -s "$SESSION" "$CMD"

for ((i = 2; i <= N; i++)); do
  tmux split-window -t "$SESSION" "$CMD"
  tmux select-layout -t "$SESSION" tiled
done

tmux attach -t "$SESSION"