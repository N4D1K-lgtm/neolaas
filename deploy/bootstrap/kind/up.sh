#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME=infra-dev

if kind get clusters | grep -q "^${CLUSTER_NAME}$"; then
  echo "Kind cluster already exists"
  exit 0
fi

cat <<EOF | kind create cluster --name ${CLUSTER_NAME} --config=-
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: neolaas
  - role: worker
EOF
