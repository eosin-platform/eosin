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
            kubectl rollout restart statefulset --context $KUBECONTEXT -n histion "histion-$arg"
            ;;
        *)
            restart_args+=("$arg")
            ;;
        esac
    done
    kubectl rollout restart deployment --context $KUBECONTEXT -n histion "${restart_args[@]/#/histion-}"
}

main() {
    do_build $@
    do_restart $@
    k9s -n histion --splashless --context $KUBECONTEXT
}

main $@