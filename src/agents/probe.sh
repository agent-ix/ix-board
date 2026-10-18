#!/bin/sh
# One batched read of a machine's agent files, panes and git state.
# Usage: probe.sh <child-window-minutes> (session|<harness>|<id> | git|<cwd>)...
# Emits @@-prefixed line records that agents::probe::parse_probe reads.
win=$1
shift

mt() { stat -c %Y "$1" 2>/dev/null || stat -f %m "$1" 2>/dev/null; }

echo "@@NOW $(date +%s)"

codex_recent=""
if [ -d "$HOME/.codex/sessions" ]; then
  codex_recent=$(find "$HOME/.codex/sessions" -name 'rollout-*.jsonl' -mmin "-$win" 2>/dev/null)
fi

for spec in "$@"; do
  type=${spec%%|*}
  rest=${spec#*|}
  case "$type" in
    session)
      harness=${rest%%|*}
      sid=${rest#*|}
      case "$sid" in '' | *[!A-Za-z0-9_-]*) continue ;; esac
      echo "@@SESSION $sid"
      file=""
      case "$harness" in
        claude)
          for f in "$HOME"/.claude/projects/*/"$sid".jsonl; do
            if [ -f "$f" ]; then file=$f; break; fi
          done
          if [ -n "$file" ]; then
            for g in "${file%.jsonl}"/subagents/agent-*.jsonl; do
              if [ -f "$g" ]; then echo "@@CHILD $(mt "$g")"; fi
            done
          fi
          ;;
        codex)
          file=$(find "$HOME/.codex/sessions" -name "rollout-*-$sid.jsonl" 2>/dev/null | head -n 1)
          for c in $codex_recent; do
            if head -n 1 "$c" | grep -q "\"parent_thread_id\":\"$sid\""; then
              echo "@@CHILD $(mt "$c")"
            fi
          done
          ;;
      esac
      if [ -n "$file" ] && [ -f "$file" ]; then
        echo "@@MTIME $(mt "$file")"
        echo "@@TAIL"
        tail -c 262144 "$file"
        echo
      fi
      echo "@@END"
      ;;
    git)
      cwd=$rest
      echo "@@GIT $cwd"
      commit=$(git -C "$cwd" log -1 --format=%ct 2>/dev/null)
      if [ -n "$commit" ]; then echo "@@COMMIT $commit"; fi
      top=$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null)
      if [ -n "$top" ]; then
        git -C "$cwd" status --porcelain 2>/dev/null | head -n 200 | sed 's/^...//' |
          {
            newest=0
            while IFS= read -r f; do
              m=$(mt "$top/$f")
              if [ -n "$m" ] && [ "$m" -gt "$newest" ]; then newest=$m; fi
            done
            if [ "$newest" -gt 0 ]; then echo "@@DIRTY $newest"; fi
          }
      fi
      echo "@@END"
      ;;
  esac
done

if [ -d /proc/self ]; then
  grep -a -z -o -H '^HERDR_PANE_ID=[^[:cntrl:]]*' /proc/[0-9]*/environ 2>/dev/null | tr '\0' '\n' |
    sed -n 's#^/proc/\([0-9]*\)/environ:HERDR_PANE_ID=\(.*\)$#@@P \1 \2#p'
fi
ps -eo pid=,ppid=,comm=,args= 2>/dev/null | sed 's/^ */@@PS /'
