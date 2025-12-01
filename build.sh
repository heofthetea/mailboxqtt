#!/bin/bash
set -e

# Configuration
IMAGE_NAME="heofthetea/mailboxqtt"
VERSION=${1:-latest}

echo "Building multi-platform Docker image: $IMAGE_NAME:$VERSION"

# Build for both x86_64 and ARM64 and push to Docker Hub
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t $IMAGE_NAME:$VERSION \
  -t $IMAGE_NAME:latest \
  --push \
  .

echo "Build complete! Images pushed to Docker Hub."
echo "Platforms: linux/amd64, linux/arm64"
