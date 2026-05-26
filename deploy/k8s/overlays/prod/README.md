# Prod overlay

Before applying:

1. Set image registry + tags:

```sh
cd deploy/k8s/overlays/prod
kustomize edit set image \
  runinator-ws=registry.example.com/runinator-ws:1.0.0 \
  runinator-scheduler=registry.example.com/runinator-scheduler:1.0.0 \
  runinator-worker=registry.example.com/runinator-worker:1.0.0 \
  runinator-importer=registry.example.com/runinator-importer:1.0.0
```

2. Edit `storage-class-patch.yaml` and replace `REPLACE_STORAGE_CLASS` with
   your cluster's StorageClass name.

3. Create the three Secrets in the namespace (see
   `deploy/k8s/base/secrets.example.yaml` — copy outside the repo, fill in,
   apply).

4. Apply:

```sh
kubectl apply -k deploy/k8s/overlays/prod
```
