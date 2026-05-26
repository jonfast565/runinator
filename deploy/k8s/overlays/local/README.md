# Local overlay

Targets a single-node local cluster (Docker Desktop, k3d, minikube, kind).
The overlay includes development-only Secrets for Postgres, RabbitMQ, and the
app credential store.

Expects a default `StorageClass` (k3d ships `local-path`; kind needs the
local-path provisioner installed; minikube enables it by default).

The preferred end-to-end command is:

```sh
pwsh ./build.ps1 -DeployKube
```

For clusters that cannot see Docker Desktop's local image store, push through a
registry reachable from the cluster:

```sh
pwsh ./build.ps1 -DeployKube -LocalRegistry localhost:5000
```

If images are already available and you only need to apply the overlay:

```sh
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
