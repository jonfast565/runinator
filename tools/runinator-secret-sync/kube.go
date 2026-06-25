package main

import (
	"bytes"
	"context"
	"fmt"

	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/tools/clientcmd"
)

// secretWriter reconciles a Secret's data, returning a short status word.
type secretWriter interface {
	apply(ctx context.Context, namespace, name string, data map[string][]byte) (string, error)
}

// kubeClient writes Secrets through client-go using the local kubeconfig, so
// exec credential plugins (e.g. EKS `aws eks get-token`) resolve transparently.
type kubeClient struct {
	clientset kubernetes.Interface
}

func newKubeClient(kubeconfig, kubeContext string) (*kubeClient, error) {
	loadingRules := clientcmd.NewDefaultClientConfigLoadingRules()
	if kubeconfig != "" {
		loadingRules.ExplicitPath = kubeconfig
	}
	overrides := &clientcmd.ConfigOverrides{}
	if kubeContext != "" {
		overrides.CurrentContext = kubeContext
	}
	restConfig, err := clientcmd.NewNonInteractiveDeferredLoadingClientConfig(loadingRules, overrides).ClientConfig()
	if err != nil {
		return nil, fmt.Errorf("load kubeconfig: %w", err)
	}
	clientset, err := kubernetes.NewForConfig(restConfig)
	if err != nil {
		return nil, fmt.Errorf("build clientset: %w", err)
	}
	return &kubeClient{clientset: clientset}, nil
}

var managedLabels = map[string]string{
	"app.kubernetes.io/managed-by": "runinator-secret-sync",
}

func (k *kubeClient) apply(ctx context.Context, namespace, name string, data map[string][]byte) (string, error) {
	secrets := k.clientset.CoreV1().Secrets(namespace)

	existing, err := secrets.Get(ctx, name, metav1.GetOptions{})
	if apierrors.IsNotFound(err) {
		secret := &corev1.Secret{
			ObjectMeta: metav1.ObjectMeta{Name: name, Namespace: namespace, Labels: managedLabels},
			Type:       corev1.SecretTypeOpaque,
			Data:       data,
		}
		if _, err := secrets.Create(ctx, secret, metav1.CreateOptions{}); err != nil {
			return "", fmt.Errorf("create: %w", err)
		}
		return "created", nil
	}
	if err != nil {
		return "", fmt.Errorf("get: %w", err)
	}

	if dataEqual(existing.Data, data) {
		return "noop", nil
	}

	existing.Data = data
	existing.Type = corev1.SecretTypeOpaque
	if existing.Labels == nil {
		existing.Labels = map[string]string{}
	}
	for key, value := range managedLabels {
		existing.Labels[key] = value
	}
	if _, err := secrets.Update(ctx, existing, metav1.UpdateOptions{}); err != nil {
		return "", fmt.Errorf("update: %w", err)
	}
	return "updated", nil
}

// dataEqual reports whether two Secret data maps are byte-for-byte identical.
func dataEqual(a, b map[string][]byte) bool {
	if len(a) != len(b) {
		return false
	}
	for key, valueA := range a {
		valueB, ok := b[key]
		if !ok || !bytes.Equal(valueA, valueB) {
			return false
		}
	}
	return true
}
