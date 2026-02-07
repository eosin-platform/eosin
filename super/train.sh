#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
source .venv/bin/activate
python train_sr.py --data-root $HOME/Repositories/camelyon/CAMELYON17/images