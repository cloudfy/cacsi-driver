# Kubernetes CSI Certificate Driver

A Kubernetes CSI (Container Storage Interface) driver that provides ephemeral volumes containing short-lived TLS certificates. Certificates are issued via a central gRPC service and automatically renewed before expiration.

## Features

- **Ephemeral Certificate Volumes**: Mount certificates as ephemeral volumes in pods
- **Automatic Certificate Issuance**: Certificates generated on volume mount via gRPC service
- **Certificate Renewal**: Background monitoring service automatically renews certificates before expiry
- **Secure CA Management**: CA certificate and key retrieved from Kubernetes secrets, stored only in memory on nodes
- **Short-lived Certificates**: Default 7-day validity with automatic renewal at 20% remaining lifetime
- **Pod-specific Certificates**: Certificates named using `$POD_NAMESPACE-$POD_NAME-$VOLUME_ID` pattern

## Architecture

### Components

1. **CSI Driver** (DaemonSet on each node)
   - Implements CSI Node Service
   - Mounts ephemeral volumes with certificates
   - Monitors certificate expiration
   - Automatically renews expiring certificates

2. **Certificate Service** (Deployment)
   - Central gRPC service for certificate operations
   - Issues and renews certificates signed by CA
   - Maintains in-memory database of issued certificates
   - Loads CA from Kubernetes secret

3. **Certificate Monitor** (Background service in CSI driver)
   - Checks certificate expiration every 5 minutes
   - Triggers renewal when < 20% of lifetime remains
   - Updates mounted certificate files automatically

## Prerequisites

- Kubernetes cluster (v1.20+)
- Rust toolchain (1.75+)
- Docker for building images
- CA certificate and key

## Building

### Build Rust binaries

```bash
cd src
cargo build --release
```

### Build Docker image

```bash
docker build -t cacsi-driver:latest .
```

### Push to registry

```bash
docker tag cacsi-driver:latest your-registry/cacsi-driver:latest
docker push your-registry/cacsi-driver:latest
```

## Deployment

### 1. Generate CA Certificate

```bash
# Generate CA certificate and key
openssl req -x509 -newkey rsa:4096 -keyout ca.key -out ca.crt \
  -days 3650 -nodes -subj "/CN=CSI-CA"

# Create Kubernetes secret
kubectl create namespace cacsi
kubectl create secret tls csi-ca-secret \
  --cert=ca.crt \
  --key=ca.key \
  -n cacsi
```

### 2. Deploy CSI Driver

```bash
# Update image in deploy/csi-driver.yaml with your registry
kubectl apply -f deploy/csi-driver.yaml
```

### 3. Verify deployment

```bash
# Check certificate service
kubectl get pods -n cacsi -l app=cacsi-service

# Check CSI driver on nodes
kubectl get pods -n cacsi -l app=cacsi-driver

# Check CSI driver registration
kubectl get csidriver csi.k8s.cacsi-driver
```

### 4. Re-release 

```bash
# rollout csi drivers
kubectl rollout restart daemonset/cacsi-driver -n cacsi

# rollout service
kubectl rollout restart deployment/cacsi-service -n cacsi
```

## Usage

### Mount certificate volume in a pod

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-app
  namespace: default
spec:
  containers:
    - name: app
      image: nginx:latest
      volumeMounts:
        - name: certs
          mountPath: /etc/certs
          readOnly: true
  volumes:
    - name: certs
      csi:
        driver: csi.k8s.cacsi-driver
        volumeAttributes:
          # Optional: specify certificate validity in days (default: 7)
          validity_days: "7"
```

The certificates will be available at:
- `/etc/certs/tls.crt` - Certificate (PEM)
- `/etc/certs/tls.key` - Private key (PEM)

### Certificate Naming

Certificates are stored on the node using the pattern:
```
$POD_NAMESPACE-$POD_NAME-$VOLUME_ID
```

Example: `default-my-app-volume-12345`

### Certificate Properties

- **Common Name**: `$POD_NAME.$POD_NAMESPACE.svc.$CLUSTER_DOMAIN` (default)
- **DNS SANs**: `$POD_NAME`
- **Organizational Units**: Optional, comma-separated list (configurable via `organizational_units` attribute)
- **Validity**: 7 days (default, configurable via `validity_days` attribute)
- **Renewal**: Automatic when < 20% lifetime remains (~1.4 days before expiry)

### Custom Common Name Template

You can customize the certificate's Common Name (CN) using template syntax that references pod metadata and spec fields:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-app
  namespace: production
spec:
  serviceAccountName: my-service-account
  containers:
    - name: app
      image: nginx:latest
      volumeMounts:
        - name: certs
          mountPath: /etc/certs
          readOnly: true
  volumes:
    - name: certs
      csi:
        driver: csi.k8s.cacsi-driver
        volumeAttributes:
          # Custom CN template using pod metadata and spec
          cn_template: "{spec.serviceAccountName}.{metadata.namespace}.svc.cluster.local"
          validity_days: "30"
```

#### Template Syntax

Templates use `{section.field}` syntax to reference pod information:

**Available metadata fields:**
- `{metadata.name}` - Pod name
- `{metadata.namespace}` - Pod namespace
- `{metadata.uid}` - Pod UID
- `{metadata.labels.<label-key>}` - Specific label value
- `{metadata.annotations.<annotation-key>}` - Specific annotation value

**Available spec fields:**
- `{spec.serviceAccountName}` - Service account name
- `{spec.nodeName}` - Node the pod is running on
- `{spec.hostname}` - Pod hostname (if set)
- `{spec.subdomain}` - Pod subdomain (if set)
- `{spec.priorityClassName}` - Pod priority class name (if set)

#### Template Examples

1. **Service Account Based CN:**
   ```yaml
   cn_template: "{spec.serviceAccountName}.{metadata.namespace}"
   # Result: my-service-account.production
   ```

2. **Label-based CN:**
   ```yaml
   cn_template: "{metadata.labels.app}.{metadata.namespace}.svc.cluster.local"
   # Result: nginx.production.svc.cluster.local
   ```

3. **Complex CN with multiple fields:**
   ```yaml
   cn_template: "{metadata.name}.{spec.serviceAccountName}.{metadata.namespace}"
   # Result: my-app.my-service-account.production
   ```

#### Behavior

- If `cn_template` is **not provided**, the default format is used: `{metadata.name}.{metadata.namespace}.svc.{cluster-domain}`
- If a template field doesn't exist (e.g., pod has no `serviceAccountName`), the certificate issuance will fail with a clear error message
- Templates are resolved at volume mount time using live pod information from the Kubernetes API

### Organizational Units

You can add one or more Organizational Units (OU) to your certificates using the `organizational_units` attribute:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-app
  namespace: production
spec:
  containers:
    - name: app
      image: nginx:latest
      volumeMounts:
        - name: certs
          mountPath: /etc/certs
          readOnly: true
  volumes:
    - name: certs
      csi:
        driver: csi.k8s.cacsi-driver
        volumeAttributes:
          # Single organizational unit
          organizational_units: "Engineering"
          validity_days: "30"
```

#### Multiple Organizational Units

To specify multiple OUs, provide them as a comma-separated list:

```yaml
volumeAttributes:
  # Multiple OUs will be added to the certificate in order
  organizational_units: "Engineering, Platform, Security"
  validity_days: "30"
```

#### Behavior

- The `organizational_units` attribute is **optional** - certificates can be issued without OUs
- Multiple OUs can be specified as comma-separated values
- Leading and trailing whitespace is automatically trimmed from each OU
- Empty OUs are ignored
- OUs appear in the certificate's Distinguished Name in the order specified
- To inspect OUs in the certificate, use: `openssl x509 -in tls.crt -text -noout`

## Configuration

### Environment Variables (CSI Driver)

- `CSI_ENDPOINT`: Unix socket path (default: `unix:///csi/csi.sock`)
- `NODE_ID`: Node identifier (default: hostname)
- `CERT_SERVICE_ADDR`: Certificate service address (default: `http://cacsi-service:50051`)
- `CA_SECRET_NAME`: CA secret name (default: `csi-ca-secret`)
- `CA_SECRET_NAMESPACE`: CA secret namespace (default: `kube-system`)
- `CERT_BASE_PATH`: Base path for certificate storage (default: `/var/lib/csi-certs`)
- `CLUSTER_DOMAIN`: Kubernetes cluster domain (default: `cluster.local`)
- `RUST_LOG`: Log level (default: `info`)

### Environment Variables (Certificate Service)

- `LISTEN_ADDR`: gRPC listen address (default: `0.0.0.0:50051`)
- `CA_SECRET_NAME`: CA secret name (default: `csi-ca-secret`)
- `CA_SECRET_NAMESPACE`: CA secret namespace (default: `kube-system`)
- `RUST_LOG`: Log level (default: `info`)

## Security Considerations

1. **CA Security**:
   - CA certificate and key stored in Kubernetes secret
   - CA loaded into memory only, never written to disk on nodes
   - CA never transmitted over network (only certificates are)

2. **Certificate Storage**:
   - Certificates stored in node local storage
   - Each pod gets unique certificate
   - Certificates automatically cleaned up on pod deletion

3. **RBAC**:
   - Service account requires access to secrets for CA retrieval
   - Minimal permissions granted to CSI driver

4. **Network Security**:
   - gRPC communication between driver and service within cluster
   - Can be secured with mTLS if needed

## Monitoring

### Check certificate service logs

```bash
kubectl logs -n cacsi -l app=cacsi-service
```

### Check CSI driver logs

```bash
kubectl logs -n cacsi -l app=cacsi-driver -c csi-driver
```

### View issued certificates

Certificates are tracked in the certificate service's in-memory database and monitored by each CSI driver instance.

## Troubleshooting

### Pod fails to mount volume

1. Check CSI driver is running on the node:
   ```bash
   kubectl get pods -n cacsi -o wide
   ```

2. Check driver logs:
   ```bash
   kubectl logs -n cacsi <cacsi-driver-pod> -c csi-driver
   ```

3. Verify CA secret exists:
   ```bash
   kubectl get secret csi-ca-secret -n cacsi
   ```

### Certificate not renewing

1. Check certificate monitor logs for renewal attempts
2. Verify certificate service is accessible from node
3. Check certificate service logs for errors

### Certificate service not starting

1. Verify CA secret exists and contains valid PEM data
2. Check RBAC permissions for service account
3. Review certificate service logs

## Development

### Project Structure

```
src/
├── main.rs                 # CSI driver entry point
├── build.rs               # Protobuf compilation
├── Cargo.toml             # Dependencies
├── proto/                 # Protocol buffer definitions
│   ├── csi.proto
│   └── cert_service.proto
├── csi/                   # CSI implementation
│   ├── identity.rs        # Identity service
│   └── node.rs           # Node service
├── cert_manager.rs        # Certificate management
├── ca_manager.rs          # CA management
├── cert_monitor.rs        # Certificate monitoring
├── k8s_client.rs         # Kubernetes client
└── cert_service/          # Certificate service
    ├── main.rs
    └── service.rs
```

### Running locally

For development, you can run the components locally (requires kubeconfig):

```bash
# Run certificate service
RUST_LOG=debug cargo run --bin cacsi-service

# Run CSI driver (requires root for socket creation)
sudo RUST_LOG=debug cargo run --bin csi-driver
```

## License

MIT

## Contributing

Contributions welcome! Please open an issue or pull request.
