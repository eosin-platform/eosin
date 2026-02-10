#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/"
KUBECONTEXT="${KUBECONTEXT:-do-nyc3-beeb}"
echo "Using kubectl context: $KUBECONTEXT"

do_build() {
    build_args=()
        for arg in "$@"; do
            case "$arg" in
            *)
                build_args+=("$arg")
                ;;
        esac
    done
    ./build.sh --push "${build_args[@]}"
}

do_restart() {
    restart_args=()
    for arg in "$@"; do
        case "$arg" in
        compiler)
            kubectl rollout restart statefulset --context $KUBECONTEXT -n eosin "eosin-compiler"
            exit 0
            ;;
        storage)
            kubectl rollout restart statefulset --context $KUBECONTEXT -n eosin "eosin-storage"
            exit 0
            ;;
        *)
            restart_args+=("$arg")
            ;;
        esac
    done
    # restart the compiler if no specific service is mentioned
    if [ ${#restart_args[@]} -eq 0 ]; then
        kubectl rollout restart statefulset --context $KUBECONTEXT -n eosin "eosin-compiler" || true
        kubectl rollout restart statefulset --context $KUBECONTEXT -n eosin "eosin-storage" || true
    fi
    kubectl rollout restart deployment --context $KUBECONTEXT -n eosin "${restart_args[@]/#/eosin-}"
}

main() {
    do_build $@
    do_restart $@
    k9s -n eosin --splashless --context $KUBECONTEXT
}

main $@