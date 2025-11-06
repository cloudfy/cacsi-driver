#!/bin/bash
set -e

IMAGE_NAME=${1:-"cacsi-driver:latest"}
REGISTRY=${2:-""}

if [ -n "$REGISTRY" ]; then
  FULL_IMAGE="$REGISTRY/$IMAGE_NAME"
else
  FULL_IMAGE="$IMAGE_NAME"
fi

echo "Building and deploying CSI Certificate Driver..."
echo "Image: $FULL_IMAGE"
echo ""

# Build Docker image
echo "Building Docker image..."
docker build -t $FULL_IMAGE .

# Push to registry if specified
if [ -n "$REGISTRY" ]; then
  echo "Pushing image to registry..."
  docker push $FULL_IMAGE
fi

# Update deployment with image
echo "Updating deployment manifests..."
sed -i.bak "s|image: cacsi-driver:latest|image: $FULL_IMAGE|g" deploy/csi-driver.yaml

# Deploy to Kubernetes
echo "Deploying to Kubernetes..."
kubectl apply -f deploy/csi-driver.yaml

# Restore original manifest
mv deploy/csi-driver.yaml.bak deploy/csi-driver.yaml

echo ""
echo "Deployment complete!"
echo ""
echo "To check status:"
echo "  kubectl get pods -n cacsi"
echo ""
echo "To view logs:"
echo "  kubectl logs -n cacsi -l app=cacsi-service"
echo "  kubectl logs -n cacsi -l app=cacsi-driver -c csi-driver"
