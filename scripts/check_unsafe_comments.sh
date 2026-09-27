#!/usr/bin/env bash
# Enforce that every `unsafe {` block in src/ and tests/ has a `// SAFETY:`
# comment within the 3 lines preceding it. Pre-existing exemptions live in the
# baseline file below; regenerate with `--update-baseline`.
#
# Both directories are scanned by default (not just src/) so that an unsafe
# block hiding in an integration test cannot slip past a scope that only ever
# pointed at src/ — the exact defect fixed in agent-ix/filament-ide-rs#398,
# where the audited directory held zero unsafe blocks and so passed
# identically whether or not any justification existed. The number of unsafe
# blocks audited is always printed on success, so a pass can be told apart
# from a scan of nothing.
set -euo pipefail

baseline_file="scripts/unsafe_comment_baseline.txt"
update_baseline=false

if [[ "${1:-}" == "--update-baseline" ]]; then
  update_baseline=true
fi

scan_dirs=()
for dir in src tests; do
  if [[ -d "$dir" ]]; then
    scan_dirs+=("$dir")
  fi
done

scan_dirs_desc="none"
if [[ ${#scan_dirs[@]} -gt 0 ]]; then
  scan_dirs_desc="${scan_dirs[*]}"
fi

unsafe_lines=()
if [[ ${#scan_dirs[@]} -gt 0 ]]; then
  while IFS= read -r line; do
    unsafe_lines+=("$line")
  done < <(grep -rEn 'unsafe[[:space:]]*\{' "${scan_dirs[@]}" 2>/dev/null || true)
fi

audited=${#unsafe_lines[@]}

missing_lines=()
if [[ "$audited" -gt 0 ]]; then
  for entry in "${unsafe_lines[@]}"; do
    file=${entry%%:*}
    rest=${entry#*:}
    line=${rest%%:*}

    start=$(( line > 3 ? line - 3 : 1 ))
    if ! sed -n "${start},${line}p" "$file" | grep -q '// SAFETY:'; then
      missing_lines+=("${file}:${line}")
    fi
  done
fi

if [[ "$update_baseline" == true ]]; then
  if [[ ${#missing_lines[@]} -eq 0 ]]; then
    : > "$baseline_file"
    echo "wrote empty ${baseline_file}"
  else
    printf '%s\n' "${missing_lines[@]}" | sort -u > "$baseline_file"
    echo "wrote ${baseline_file} with ${#missing_lines[@]} entries"
  fi
  echo "check_unsafe_comments: audited ${audited} unsafe block(s) in ${scan_dirs_desc}"
  exit 0
fi

if [[ ${#missing_lines[@]} -eq 0 ]]; then
  echo "check_unsafe_comments: audited ${audited} unsafe block(s) in ${scan_dirs_desc}"
  exit 0
fi

if [[ ! -f "$baseline_file" ]]; then
  echo "missing unsafe comment baseline: ${baseline_file}" >&2
  echo "run: bash scripts/check_unsafe_comments.sh --update-baseline" >&2
  exit 1
fi

missing=0
for entry in "${missing_lines[@]}"; do
  if ! grep -Fxq "$entry" "$baseline_file"; then
    echo "missing SAFETY comment near ${entry}" >&2
    missing=1
  fi
done

if [[ "$missing" -eq 0 ]]; then
  echo "check_unsafe_comments: audited ${audited} unsafe block(s) in ${scan_dirs_desc} (baseline-covered)"
fi

exit "$missing"
