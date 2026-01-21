#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLUSTER_NAME=infra-dev

if kind get clusters | grep -q "^${CLUSTER_NAME}$"; then
	echo "Kind cluster '${CLUSTER_NAME}' already exists"
	exit 0
fi

echo "Creating Kind cluster '${CLUSTER_NAME}'..."
kind create cluster --name "${CLUSTER_NAME}" --config="${SCRIPT_DIR}/kind-config.yaml"

echo "Kind cluster '${CLUSTER_NAME}' created successfully"
