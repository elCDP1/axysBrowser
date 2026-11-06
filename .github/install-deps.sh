#!/usr/bin/env bash
# Install the GTK4/WebKitGTK development stack used by the Linux CI jobs.
#
# The hosted runners occasionally use a slow apt mirror. A plain
# `apt-get install` then burns the whole job timeout and the job is reported
# as cancelled. Never let debconf prompt, and bound one install with a
# 40-minute cap: a healthy but slow WebKitGTK download needs more than
# 15 minutes, while a real failure still fails instead of hanging forever.
# apt's own Acquire::Retries handles transient network failures.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

packages=(
  libgtk-4-dev
  libwebkitgtk-6.0-dev
  libjavascriptcoregtk-6.0-dev
  pkg-config
  "$@"
)

apt_options=(
  -o Acquire::Retries=5
  -o Acquire::http::Timeout=60
  -o Acquire::https::Timeout=60
)

sudo timeout --kill-after=30 300 apt-get "${apt_options[@]}" update
sudo timeout --kill-after=30 2400 apt-get "${apt_options[@]}" install -y --no-install-recommends "${packages[@]}"
