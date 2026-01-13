#!/usr/bin/env bash
set -euo pipefail

LOG="${1:?usage: fault_tamper_hash.sh <logfile>}"

python3 - << 'PY'
import sys, json
p=sys.argv[1]
lines=open(p,'r',encoding='utf-8').read().splitlines()
for i,l in enumerate(lines):
    if not l.strip(): 
        continue
    o=json.loads(l)
    if "hash" in o and isinstance(o["hash"], list) and len(o["hash"])>=1:
        o["hash"][0]=(o["hash"][0]+1)%256
        lines[i]=json.dumps(o, separators=(",",":"))
        break
open(p,'w',encoding='utf-8').write("\n".join(lines)+"\n")
print("tampered:", p)
PY "${LOG}"
