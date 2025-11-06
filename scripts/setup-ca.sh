#!/bin/bash
set -e

NAMESPACE="csi-cert-system"
CA_SECRET="csi-ca-secret"

echo "Generating CA Certificate for CSI Driver..."

# Create temporary directory
TEMP_DIR=$(mktemp -d)
cd $TEMP_DIR

# Generate CA private key
echo "Generating CA private key..."
openssl genrsa -out ca.key 4096

# Generate CA certificate
echo "Generating CA certificate..."
openssl req -x509 -new -nodes -key ca.key -sha256 -days 3650 \
  -out ca.crt \
  -subj "/C=US/ST=State/L=City/O=Organization/CN=CSI-Certificate-Authority"

echo ""
echo "CA Certificate generated successfully!"
echo "Certificate: $TEMP_DIR/ca.crt"
echo "Private Key: $TEMP_DIR/ca.key"
echo ""

# Create namespace if it doesn't exist
echo "Creating namespace $NAMESPACE..."
kubectl create namespace $NAMESPACE --dry-run=client -o yaml | kubectl apply -f -

# Create or update secret
echo "Creating secret $CA_SECRET in namespace $NAMESPACE..."
kubectl create secret tls $CA_SECRET \
  --cert=ca.crt \
  --key=ca.key \
  -n $NAMESPACE \
  --dry-run=client -o yaml | kubectl apply -f -

echo ""
echo "Setup complete!"
echo ""
echo "CA certificate and key have been stored in Kubernetes secret: $NAMESPACE/$CA_SECRET"
echo ""
echo "To verify the secret:"
echo "  kubectl get secret $CA_SECRET -n $NAMESPACE"
echo ""
echo "To view the CA certificate:"
echo "  kubectl get secret $CA_SECRET -n $NAMESPACE -o jsonpath='{.data.tls\.crt}' | base64 -d"
echo ""
echo "Temporary files are in: $TEMP_DIR"
echo "Remember to securely delete these files after verification:"
echo "  rm -rf $TEMP_DIR"
