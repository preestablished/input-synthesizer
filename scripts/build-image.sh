#!/usr/bin/env bash
# Build the input-synthesizer container image (v1 deployment artifact).
#
# Stages a two-repo build context (this repo at HEAD + control-plane at tag
# proto-v0.2.0) so the ../control-plane path dependency resolves inside the
# Docker build — see the Dockerfile header.
#
# Usage:
#   scripts/build-image.sh [--multi-arch] [TAG]
#
#   TAG           image tag (default: input-synthesizer:dev)
#   --multi-arch  use docker buildx for linux/amd64 + linux/arm64
#                 (requires a configured buildx builder; single-arch local
#                 builds are the default)
#
# Records the source SHAs on stdout; the image digest is printed by docker.
set -euo pipefail

MULTI_ARCH=0
TAG="input-synthesizer:dev"
for arg in "$@"; do
  case "$arg" in
    --multi-arch) MULTI_ARCH=1 ;;
    *) TAG="$arg" ;;
  esac
done

REPO_ROOT="$(git rev-parse --show-toplevel)"
CONTROL_PLANE="${CONTROL_PLANE_DIR:-$REPO_ROOT/../control-plane}"
PROTO_TAG="${PROTO_TAG:-proto-v0.2.0}"

if [ ! -d "$CONTROL_PLANE/.git" ]; then
  echo "error: control-plane checkout not found at $CONTROL_PLANE" >&2
  echo "       (set CONTROL_PLANE_DIR to override)" >&2
  exit 1
fi

SYNTH_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD)"
CP_SHA="$(git -C "$CONTROL_PLANE" rev-parse "${PROTO_TAG}^{commit}")"
echo "input-synthesizer @ $SYNTH_SHA"
echo "control-plane     @ $CP_SHA (tag $PROTO_TAG)"
if ! git -C "$REPO_ROOT" diff --quiet HEAD; then
  echo "warning: input-synthesizer working tree is dirty; image builds from HEAD" >&2
fi

CTX="$(mktemp -d)"
trap 'rm -rf "$CTX"' EXIT
mkdir -p "$CTX/input-synthesizer" "$CTX/control-plane"
git -C "$REPO_ROOT" archive HEAD | tar -x -C "$CTX/input-synthesizer"
git -C "$CONTROL_PLANE" archive "$PROTO_TAG" | tar -x -C "$CTX/control-plane"

if [ "$MULTI_ARCH" = "1" ]; then
  docker buildx build \
    --platform linux/amd64,linux/arm64 \
    -f "$CTX/input-synthesizer/Dockerfile" \
    -t "$TAG" \
    "$CTX"
else
  docker build \
    -f "$CTX/input-synthesizer/Dockerfile" \
    -t "$TAG" \
    "$CTX"
fi

echo "built $TAG (synth $SYNTH_SHA, control-plane $CP_SHA)"
