#!/bin/bash
#
# Graceful Shutdown Script for Neolaas P2P Network
#
# This script is called by the Kubernetes preStop hook before SIGTERM is sent.
# It ensures zero-downtime shutdown by:
# 1. Signaling the application to revoke its etcd lease
# 2. Waiting for the DELETE event to propagate to all peers (5 seconds)
# 3. Allowing SIGTERM to proceed with actor system shutdown
#
# The DiscoveryController handles the actual lease revocation when it receives SIGTERM.
# This script exists as a marker for the lifecycle hook, but the actual shutdown
# logic is handled within the Rust application.

set -e

echo "[$(date -u +"%Y-%m-%dT%H:%M:%SZ")] preStop hook: Initiating graceful shutdown"

# In a more sophisticated setup, we could send a signal to the application
# to initiate shutdown before SIGTERM. For now, the application handles
# SIGTERM gracefully on its own.

# The application's DiscoveryController will:
# 1. Revoke the etcd lease (immediate DELETE event)
# 2. Wait 5 seconds for propagation
# 3. Shutdown the actor system

# We sleep for a short duration to ensure the application has time to start
# processing the shutdown before Kubernetes sends SIGTERM
sleep 1

echo "[$(date -u +"%Y-%m-%dT%H:%M:%SZ")] preStop hook: Ready for SIGTERM"

exit 0
