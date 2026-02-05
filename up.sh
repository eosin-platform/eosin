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
            kubectl rollout restart statefulset --context $KUBECONTEXT -n histion "histion-compiler"
            exit 0
            ;;
        storage)
            kubectl rollout restart statefulset --context $KUBECONTEXT -n histion "histion-storage"
            exit 0
            ;;
        *)
            restart_args+=("$arg")
            ;;
        esac
    done
    # restart the compiler if no specific service is mentioned
    if [ ${#restart_args[@]} -eq 0 ]; then
        kubectl rollout restart statefulset --context $KUBECONTEXT -n histion "histion-compiler"
        kubectl rollout restart statefulset --context $KUBECONTEXT -n histion "histion-storage"
    fi
    kubectl rollout restart deployment --context $KUBECONTEXT -n histion "${restart_args[@]/#/histion-}"
}

main() {
    do_build $@
    do_restart $@
    k9s -n histion --splashless --context $KUBECONTEXT
}

main $@