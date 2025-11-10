# build the cacsi-driver Docker image
docker build -t cacsi-driver:latest .

# tag for github container registry
docker tag cacsi-driver:latest ghcr.io/cloudfy/cacsi-driver:latest

# push to github
docker push ghcr.io/cloudfy/cacsi-driver:latest