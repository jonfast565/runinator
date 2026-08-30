#!/usr/bin/env bash
# Shared lifecycle helpers for the Kubernetes port-forward launchers.

port_forward_validate_reconnect_delay() {
  local reconnect_delay="$1"

  if ! [[ "$reconnect_delay" =~ ^([0-9]+([.][0-9]+)?|[.][0-9]+)$ ]] \
    || ! awk -v value="$reconnect_delay" 'BEGIN { exit !(value > 0) }'; then
    echo "--reconnect-delay must be a positive number of seconds" >&2
    exit 2
  fi
}

port_forward_forever() {
  local reconnect_delay="$1"
  shift
  local exit_status

  trap 'exit 0' INT TERM

  while :; do
    if "$@"; then
      exit_status=0
    else
      exit_status=$?
    fi

    echo "Port-forward exited with status ${exit_status}; reconnecting in ${reconnect_delay}s. Ctrl+C to stop." >&2
    sleep "$reconnect_delay"
  done
}
