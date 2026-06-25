package main

import (
	"context"
	"log"
)

// engine runs every job once per tick: refresh+read the source, then reconcile
// the bundle into each sink. one job's failure does not abort the others.
type engine struct {
	cfg    *Config
	kube   secretWriter
	dryRun bool
}

func (e *engine) tick(ctx context.Context) {
	for i := range e.cfg.Jobs {
		e.runJob(ctx, &e.cfg.Jobs[i])
	}
}

func (e *engine) runJob(ctx context.Context, job *Job) {
	b, err := produce(job)
	if err != nil {
		log.Printf("%s %s: %v", ts(), job.Name, err)
		return
	}
	if len(b) == 0 {
		log.Printf("%s %s: source produced nothing (skipping)", ts(), job.Name)
		return
	}

	fp := fingerprint(b)
	for i := range job.Sinks {
		sink := &job.Sinks[i]
		status, err := deliver(ctx, sink, b, e.kube, e.dryRun)
		if err != nil {
			log.Printf("%s %s -> %s: %v", ts(), job.Name, sinkLabel(sink), err)
			continue
		}
		log.Printf("%s %s [%s] %s -> %s", ts(), job.Name, fp, status, sinkLabel(sink))
	}
}

func sinkLabel(sink *Sink) string {
	switch sink.Type {
	case "kube-secret":
		return "secret/" + sink.Namespace + "/" + sink.Name
	case "file":
		return "file:" + sink.Path
	default:
		return sink.Type
	}
}
