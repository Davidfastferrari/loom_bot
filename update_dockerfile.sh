#!/bin/bash
set -e

# Backup the original Dockerfile
cp Dockerfile Dockerfile.bak

# Replace the Dockerfile with the fixed version
cp Dockerfile.fixed Dockerfile

echo "Dockerfile has been updated with fixes."