#!/usr/bin/env bash
set -euo pipefail

LOG="${1:?usage: fault_omit_line.sh <logfile>}"

python3 - << 'PY'
import sys
p=sys.argv[1]
lines=open(p,'r',encoding='utf-8').read().splitlines()
# remove first non-empty line
for i,l in enumerate(lines):
    if l.strip():
        del lines[i]
        break
open(p,'w',encoding='utf-8').write("\n".join(lines)+"\n")
print("omitted one line:", p)
PY "${LOG}"
