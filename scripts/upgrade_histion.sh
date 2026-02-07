#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
helm upgrade \
    --kube-context do-nyc3-beeb \
    --install \
    eosin \
    chart/ \
    -n eosin \
    -f scripts/eosin_values.yaml
