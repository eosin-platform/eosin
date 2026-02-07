#!/bin/bash
# Run TensorBoard in Docker with the runs directory mounted

LOGDIR="${1:-./runs_vae}"
PORT="${2:-6006}"

docker run --rm -it \
    -p "${PORT}:6006" \
    -v "$(realpath "${LOGDIR}"):/logs:ro" \
    tensorflow/tensorflow:latest \
    tensorboard --logdir=/logs --bind_all
