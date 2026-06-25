package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

// Config is the on-disk spec: a list of independent sync jobs.
type Config struct {
	// default namespace applied to kube-secret sinks that omit their own.
	Namespace string `json:"namespace,omitempty"`
	Jobs      []Job  `json:"jobs"`
}

// Job pulls one credential bundle from a source (optionally refreshing it first)
// and reconciles it into one or more sinks.
type Job struct {
	Name    string   `json:"name"`
	Refresh *Refresh `json:"refresh,omitempty"`
	Source  Source   `json:"source"`
	Sinks   []Sink   `json:"sinks"`
}

// Refresh runs `run` before reading the source, unless the `unless` probe
// command exits zero. This is the generic shape behind, e.g., "run `aws sso
// login` unless `aws sts get-caller-identity` already succeeds".
type Refresh struct {
	Run    []string `json:"run"`
	Unless []string `json:"unless,omitempty"`
}

// Source produces the credential bytes. Exactly one shape is used per `type`:
//   - command: run argv, capture stdout as a single blob
//   - file:    read one file as a single blob
//   - dir:     read every file matching glob as a blob keyed by file name
type Source struct {
	Type string   `json:"type"`
	Run  []string `json:"run,omitempty"`  // command
	Path string   `json:"path,omitempty"` // file, dir
	Glob string   `json:"glob,omitempty"` // dir (default "*")
}

// Sink delivers the bundle. One shape per `type`:
//   - kube-secret: create/update a Secret's data keys
//   - file:        write a single-blob bundle to a path (0600)
type Sink struct {
	Type string `json:"type"`

	// kube-secret
	Namespace string `json:"namespace,omitempty"`
	Name      string `json:"name,omitempty"`
	Key       string `json:"key,omitempty"` // data key for a single-blob source

	// file
	Path string `json:"path,omitempty"`
}

func loadConfig(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read config %s: %w", path, err)
	}
	var cfg Config
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&cfg); err != nil {
		return nil, fmt.Errorf("parse config %s: %w", path, err)
	}
	if err := cfg.validate(); err != nil {
		return nil, fmt.Errorf("invalid config %s: %w", path, err)
	}
	return &cfg, nil
}

func (c *Config) validate() error {
	if len(c.Jobs) == 0 {
		return fmt.Errorf("no jobs defined")
	}
	seen := map[string]bool{}
	for i := range c.Jobs {
		job := &c.Jobs[i]
		if job.Name == "" {
			return fmt.Errorf("job %d: missing name", i)
		}
		if seen[job.Name] {
			return fmt.Errorf("duplicate job name %q", job.Name)
		}
		seen[job.Name] = true
		if err := job.validate(c.Namespace); err != nil {
			return fmt.Errorf("job %q: %w", job.Name, err)
		}
	}
	return nil
}

func (j *Job) validate(defaultNamespace string) error {
	switch j.Source.Type {
	case "command":
		if len(j.Source.Run) == 0 {
			return fmt.Errorf("source command needs a non-empty run")
		}
	case "file":
		if j.Source.Path == "" {
			return fmt.Errorf("source file needs a path")
		}
	case "dir":
		if j.Source.Path == "" {
			return fmt.Errorf("source dir needs a path")
		}
	default:
		return fmt.Errorf("unknown source type %q (want command|file|dir)", j.Source.Type)
	}
	if j.Refresh != nil && len(j.Refresh.Run) == 0 {
		return fmt.Errorf("refresh needs a non-empty run")
	}
	if len(j.Sinks) == 0 {
		return fmt.Errorf("no sinks")
	}
	for k := range j.Sinks {
		sink := &j.Sinks[k]
		switch sink.Type {
		case "kube-secret":
			if sink.Namespace == "" {
				sink.Namespace = defaultNamespace
			}
			if sink.Namespace == "" {
				return fmt.Errorf("sink %d: kube-secret needs a namespace (or set top-level namespace)", k)
			}
			if sink.Name == "" {
				return fmt.Errorf("sink %d: kube-secret needs a name", k)
			}
		case "file":
			if sink.Path == "" {
				return fmt.Errorf("sink %d: file needs a path", k)
			}
		default:
			return fmt.Errorf("sink %d: unknown type %q (want kube-secret|file)", k, sink.Type)
		}
	}
	return nil
}

// expandPath expands a leading ~ to the user home directory.
func expandPath(path string) string {
	if path == "~" || len(path) >= 2 && path[:2] == "~/" {
		if home, err := os.UserHomeDir(); err == nil {
			return filepath.Join(home, path[1:])
		}
	}
	return path
}
