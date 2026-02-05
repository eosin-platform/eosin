#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
helm upgrade \
    --kube-context do-nyc3-beeb \
    --install \
    histion \
    chart/ \
    -n histion \
    -f scripts/histion_values.yaml
