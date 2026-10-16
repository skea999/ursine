#!/bin/sh
# memory.sh - session memory wrapper
# usage: ./memory.sh <cmd> [args]

DIR=$(cd "$(dirname "$0")" && pwd)
export AGENT_MEMORY_DIR="${AGENT_MEMORY_DIR:-.agent-memory}"

CMD=$1; shift

case "$CMD" in
  save)     "$DIR/save.sh" "$@" ;;
  recall)   "$DIR/recall.sh" "$@" ;;
  context)  DAYS=3; LIMIT=20
            while [ $# -gt 0 ]; do case "$1" in --days) DAYS=$2; shift;; --limit) LIMIT=$2; shift;; esac; shift; done
            "$DIR/context.sh" -Days "$DAYS" -Limit "$LIMIT" ;;
  daily)    "$DIR/daily.sh" "$@" ;;
  topics)   "$DIR/topics.sh" ;;
  consolidate) "$DIR/consolidate.sh" "$@" ;;
  stats)    "$DIR/stats.sh" ;;
  edit)     "$DIR/edit.sh" "$@" ;;
  export)   "$DIR/export.sh" "$@" ;;
  import)   "$DIR/import.sh" "$@" ;;
  prune)    "$DIR/prune.sh" "$@" ;;
  *)        echo "usage: ./memory.sh <save|recall|context|stats|...> [args]"; exit 1 ;;
esac