#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
cmd="docker buildx bake --builder bk --push $@"
echo ">>> $cmd"
$cmd
