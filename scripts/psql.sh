#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
: "${CONTEXT:=do-nyc3-beeb}"
secret() {
    kubectl get secret --context $CONTEXT -n eosin postgres-cred -o json \
    | jq .data.$1 \
    | xargs echo \
    | base64 -d
}
export POSTGRES_HOST=$(secret host)
export POSTGRES_USER=$(secret username)
export POSTGRES_PASSWORD=$(secret password)
export POSTGRES_PORT=$(secret port)
export POSTGRES_DB=$(secret database)
export POSTGRES_SSL_MODE=$(secret sslmode)
docker run -it --rm postgres:14 \
    psql "host=$POSTGRES_HOST port=$POSTGRES_PORT user=$POSTGRES_USER password=$POSTGRES_PASSWORD dbname=$POSTGRES_DB sslmode=$POSTGRES_SSL_MODE" \
    $@
