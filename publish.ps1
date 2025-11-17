# build the cacsi-driver Docker image
docker build -t cacsi-driver:latest .

# tag for github container registry
docker tag cacsi-driver:latest ghcr.io/cloudfy/cacsi-driver:latest
docker tag cacsi-driver:latest ghcr.io/cloudfy/cacsi-driver:1.0.12

# push to github
docker push ghcr.io/cloudfy/cacsi-driver:latest
docker push ghcr.io/cloudfy/cacsi-driver:1.0.12# Delete and recreate the test pod
kubectl delete pod test-cert-volume -n sandbox
kubectl apply -f deploy/test-pod.yaml

# Wait for pod to be ready
kubectl wait --for=condition=Ready pod/test-cert-volume -n sandbox --timeout=60s

# Verify all OUs are present
kubectl exec test-cert-volume -n sandbox -- cat /certs/tls.crt | openssl x509 -text -noout | grep "OU ="