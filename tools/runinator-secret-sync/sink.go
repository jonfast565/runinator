package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sort"
)

// deliver reconciles a bundle into a sink and returns a short status word
// ("created"/"updated"/"noop"/"dry-run") for logging.
func deliver(ctx context.Context, sink *Sink, b bundle, kube secretWriter, dryRun bool) (string, error) {
	switch sink.Type {
	case "kube-secret":
		return deliverKube(ctx, sink, b, kube, dryRun)
	case "file":
		return deliverFile(sink, b, dryRun)
	default:
		return "", fmt.Errorf("unknown sink type %q", sink.Type)
	}
}

// deliverKube maps the bundle to Secret data keys. multi-blob sources (dir) use
// their own file names; a single-blob source uses the sink's `key`.
func deliverKube(ctx context.Context, sink *Sink, b bundle, kube secretWriter, dryRun bool) (string, error) {
	data, err := keyedData(b, sink.Key)
	if err != nil {
		return "", err
	}
	if dryRun {
		return "dry-run", nil
	}
	return kube.apply(ctx, sink.Namespace, sink.Name, data)
}

// deliverFile writes a single-blob bundle to a path atomically (0600).
func deliverFile(sink *Sink, b bundle, dryRun bool) (string, error) {
	if len(b) != 1 {
		return "", fmt.Errorf("file sink needs a single-blob source, got %d", len(b))
	}
	var payload []byte
	for _, v := range b {
		payload = v
	}
	path := expandPath(sink.Path)
	if existing, err := os.ReadFile(path); err == nil && bytes.Equal(existing, payload) {
		return "noop", nil
	}
	if dryRun {
		return "dry-run", nil
	}
	if err := writeFileAtomic(path, payload); err != nil {
		return "", err
	}
	return "wrote", nil
}

// keyedData resolves bundle blobs to Secret data keys, requiring an explicit
// sink key only when a single-blob source has no natural file name.
func keyedData(b bundle, key string) (map[string][]byte, error) {
	data := map[string][]byte{}
	for name, value := range b {
		switch {
		case name != "":
			data[name] = value
		case key != "":
			data[key] = value
		default:
			return nil, fmt.Errorf("single-blob source needs the kube-secret sink to set `key`")
		}
	}
	if len(data) == 0 {
		return nil, fmt.Errorf("source produced no data")
	}
	return data, nil
}

// fingerprint hashes a bundle for logging without exposing the secret bytes.
func fingerprint(b bundle) string {
	keys := make([]string, 0, len(b))
	for k := range b {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	h := sha256.New()
	for _, k := range keys {
		h.Write([]byte(k))
		h.Write([]byte{0})
		h.Write(b[k])
		h.Write([]byte{0})
	}
	return hex.EncodeToString(h.Sum(nil))[:12]
}

// writeFileAtomic writes data to path with 0600 perms via a temp file + rename.
func writeFileAtomic(path string, data []byte) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".tmp-*")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName)
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Chmod(0o600); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmpName, path)
}
