package main

import (
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// bundle is a set of named blobs a source produces and a sink delivers. a
// command/file source yields one entry; a dir source yields one per file.
type bundle map[string][]byte

// produce runs the job's optional refresh, then reads its source into a bundle.
func produce(job *Job) (bundle, error) {
	if job.Refresh != nil {
		if err := runRefresh(job.Refresh); err != nil {
			return nil, err
		}
	}
	switch job.Source.Type {
	case "command":
		return fromCommand(job.Source.Run)
	case "file":
		return fromFile(job.Source.Path)
	case "dir":
		return fromDir(job.Source.Path, job.Source.Glob)
	default:
		return nil, fmt.Errorf("unknown source type %q", job.Source.Type)
	}
}

// runRefresh runs `run` unless the `unless` probe exits zero. stdio is inherited
// so an interactive step (e.g. `aws sso login`) can prompt at the workstation.
func runRefresh(refresh *Refresh) error {
	if len(refresh.Unless) > 0 {
		probe := exec.Command(refresh.Unless[0], refresh.Unless[1:]...)
		if probe.Run() == nil {
			return nil // probe succeeded; no refresh needed.
		}
	}
	cmd := exec.Command(refresh.Run[0], refresh.Run[1:]...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("refresh %q: %w", strings.Join(refresh.Run, " "), err)
	}
	return nil
}

// fromCommand runs argv and captures stdout as a single unnamed blob (the sink's
// key names it). a non-zero exit is an error, surfacing stderr.
func fromCommand(argv []string) (bundle, error) {
	cmd := exec.Command(argv[0], argv[1:]...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("command %q: %w: %s", strings.Join(argv, " "), err, strings.TrimSpace(stderr.String()))
	}
	if stdout.Len() == 0 {
		return nil, fmt.Errorf("command %q produced no output", strings.Join(argv, " "))
	}
	return bundle{"": stdout.Bytes()}, nil
}

func fromFile(path string) (bundle, error) {
	path = expandPath(path)
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read %s: %w", path, err)
	}
	return bundle{filepath.Base(path): data}, nil
}

// fromDir reads every file matching glob (default "*") in the directory, keyed by
// base name. a missing directory yields an empty bundle so a not-yet-populated
// source (e.g. before first login) is handled gracefully by the caller.
func fromDir(path, glob string) (bundle, error) {
	path = expandPath(path)
	if glob == "" {
		glob = "*"
	}
	entries, err := os.ReadDir(path)
	if err != nil {
		if os.IsNotExist(err) {
			return bundle{}, nil
		}
		return nil, fmt.Errorf("read dir %s: %w", path, err)
	}
	out := bundle{}
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		match, err := filepath.Match(glob, entry.Name())
		if err != nil {
			return nil, fmt.Errorf("bad glob %q: %w", glob, err)
		}
		if !match {
			continue
		}
		data, err := os.ReadFile(filepath.Join(path, entry.Name()))
		if err != nil {
			return nil, fmt.Errorf("read %s: %w", entry.Name(), err)
		}
		out[entry.Name()] = data
	}
	return out, nil
}
