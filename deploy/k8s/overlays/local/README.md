# Local overlay

Targets a single-node local cluster (k3d, minikube, kind). Expects:

- A default `StorageClass` (k3d ships `local-path`; kind needs the local-path provisioner installed; minikube enables it by default).
- Locally built images loaded into the cluster by name, e.g.:

```sh
docker build -t runinator-ws:dev        -f runinator-ws/Dockerfile        .
docker build -t runinator-scheduler:dev -f runinator-scheduler/Dockerfile .
docker build -t runinator-worker:dev    -f runinator-worker/Dockerfile    .
docker build -t runinator-importer:dev  -f runinator-importer/Dockerfile  .
docker build -t runinator-migration:dev -f runinator-migration/Dockerfile .

k3d image import runinator-ws:dev runinator-scheduler:dev \
                 runinator-worker:dev runinator-importer:dev \
                 runinator-migration:dev \
                 -c runinator
```

Apply:

```sh
kubectl apply -f deploy/k8s/base/secrets.example.yaml  # edit first!
kubectl apply -k deploy/k8s/overlays/local
```

## hostPath fallback

If your local cluster has no storage provisioner, replace the StatefulSets'
`volumeClaimTemplates` with hostPath volumes by adding this JSON patch to
`kustomization.yaml`:

```yaml
patches:
  - target: { kind: StatefulSet, name: runinator-postgres }
    patch: |-
      - op: remove
        path: /spec/volumeClaimTemplates
      - op: add
        path: /spec/template/spec/volumes
        value:
          - name: postgres-data
            hostPath:
              path: /var/runinator/postgres
              type: DirectoryOrCreate
```

(Repeat for `runinator-rabbitmq` with `rabbitmq-data` and `/var/runinator/rabbitmq`.)
