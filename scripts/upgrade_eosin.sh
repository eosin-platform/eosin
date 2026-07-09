#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
KUBE_CONTEXT=${KUBE_CONTEXT:-do-nyc3-beeb}
VALUES_PATH=${VALUES_PATH:-scripts/eosin_values.yaml}
helm upgrade \
    --kube-context "$KUBE_CONTEXT" \
    --install \
    eosin \
    chart/ \
    -n eosin \
    -f "$VALUES_PATH"
