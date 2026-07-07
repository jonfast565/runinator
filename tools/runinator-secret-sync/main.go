// runinator-secret-sync is a host helper that keeps credentials fresh inside a
// Kubernetes cluster. It is fully config-driven: a JSON spec lists independent
// jobs, each pulling a credential from a source (an arbitrary command, a file,
// or a directory) — optionally refreshing it first by running a command unless a
// probe already succeeds — and reconciling the result into one or more sinks (a
// namespaced Secret or a local file). It has no built-in knowledge of any
// particular credential; Claude, AWS SSO, GitHub tokens, etc. are just config.
//
// It authenticates to the cluster through the local kubeconfig (so EKS exec-auth
// works), and it is never built into a container image — it runs on a
// developer/operator workstation that can perform any interactive refresh and
// reach the cluster.
package main

import (
	"context"
	"flag"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"
)

func main() {
	log.SetFlags(0)

	var (
		configPath  = flag.String("config", "secret-sync.json", "path to the JSON job spec")
		interval    = flag.Duration("interval", 60*time.Second, "time between sync passes in watch mode")
		once        = flag.Bool("once", false, "run a single pass, then exit")
		dryRun      = flag.Bool("dry-run", false, "compute and log changes but write nothing")
		kubeconfig  = flag.String("kubeconfig", defaultKubeconfig(), "path to kubeconfig")
		kubeContext = flag.String("context", "", "kubeconfig context (default: current-context)")
		namespace   = flag.String("namespace", "", "override the default namespace for kube-secret sinks")
	)
	flag.Parse()

	cfg, err := loadConfig(*configPath)
	if err != nil {
		log.Fatal(err)
	}
	if *namespace != "" {
		applyNamespaceOverride(cfg, *namespace)
	}

	var kube secretWriter
	if !*dryRun && configNeedsKube(cfg) {
		client, err := newKubeClient(*kubeconfig, *kubeContext)
		if err != nil {
			log.Fatalf("kube client: %v", err)
		}
		kube = client
	}

	eng := &engine{cfg: cfg, kube: kube, dryRun: *dryRun}

	if *once {
		if !eng.tick(context.Background()) {
			os.Exit(1)
		}
		return
	}

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	log.Printf("watching every %s (config=%s jobs=%d dry-run=%v)", *interval, *configPath, len(cfg.Jobs), *dryRun)
	eng.tick(ctx)
	ticker := time.NewTicker(*interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			log.Printf("%s shutting down", ts())
			return
		case <-ticker.C:
			eng.tick(ctx)
		}
	}
}

func defaultKubeconfig() string {
	if path := os.Getenv("KUBECONFIG"); path != "" {
		return path
	}
	if home, err := os.UserHomeDir(); err == nil {
		return home + "/.kube/config"
	}
	return ""
}

// configNeedsKube reports whether any sink targets the cluster, so a kube client
// is only built (and required) when at least one kube-secret sink exists.
func configNeedsKube(cfg *Config) bool {
	for i := range cfg.Jobs {
		for j := range cfg.Jobs[i].Sinks {
			if cfg.Jobs[i].Sinks[j].Type == "kube-secret" {
				return true
			}
		}
	}
	return false
}

// applyNamespaceOverride forces every kube-secret sink into the given namespace.
func applyNamespaceOverride(cfg *Config, namespace string) {
	for i := range cfg.Jobs {
		for j := range cfg.Jobs[i].Sinks {
			if cfg.Jobs[i].Sinks[j].Type == "kube-secret" {
				cfg.Jobs[i].Sinks[j].Namespace = namespace
			}
		}
	}
}

func ts() string { return time.Now().Format(time.RFC3339) }
