g#!/bin/bash
set -e

# Build the Docker image
echo "Building Docker image..."
docker build -t loom-base:latest .

# Check if the image was built successfully
if [ $? -eq 0 ]; then
  echo "Docker image built successfully!"
  echo "To run the container:"
  echo "  docker run -d --name loom-base -v \$(pwd)/config.toml:/app/config.toml loom-base:latest"
  echo ""
  echo "To use docker-compose:"
  echo "  docker-compose up -d"
else
  echo "Failed to build Docker image."
  exit 1
fi