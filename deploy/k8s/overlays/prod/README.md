# Prod overlay

Before applying:

1. Build and push images, then render and apply this overlay with matching tags:

```sh
pwsh ./build.ps1 -DeployKube \
  -KubeManifest deploy/k8s/overlays/prod \
  -KubeContext my-prod-context \
  -ImageRepository registry.example.com/runinator \
  -ImageTag 1.0.0
```

2. Edit `storage-class-patch.yaml` and replace `REPLACE_STORAGE_CLASS` with
   your cluster's StorageClass name.

   Install [metrics-server](https://github.com/kubernetes-sigs/metrics-server)
   so the HorizontalPodAutoscalers can read CPU. ws, worker, waker, and the
   command-center web frontend each ship an HPA (bounds in
   `autoscaling-patch.yaml`) plus a PodDisruptionBudget; tune the min/max there.
   All four are safe to scale arbitrarily — ws competes on durable claims/leases
   and fans UI events over the broker, workers/wakers are broker
   competing-consumers, and the frontend is stateless. Manual scaling also works:
   `kubectl -n runinator scale deploy/runinator-ws --replicas=N`.

3. Create the three Secrets in the namespace (see
   `deploy/k8s/base/secrets.example.yaml` — copy outside the repo, fill in,
   apply).

4. Apply:

```sh
kubectl apply -k deploy/k8s/overlays/prod
```

Manual `kubectl apply -k` expects image names in `kustomization.yaml` to have
already been changed from the `REPLACE_REGISTRY/...` placeholders. The
PowerShell deploy path renders those image changes in `target/k8s-render`
without modifying the checked-in overlay.

Launch the Tauri command center through a local port-forward to the prod
Service with:

```sh
bash scripts/run-k8s.sh ui --context my-prod-context --namespace runinator
```

To browse the raw API or Scalar docs directly from the prod cluster:

```sh
bash scripts/port-forward-ws.sh --context my-prod-context --namespace runinator
```
