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

# GitHub's ubuntu-24.04 image points apt at an Azure mirror. Plain HTTP there
# intermittently stops answering on hosted runners, while archive.ubuntu.com
# does not resolve; prefer the Azure mirror over HTTPS. If it is still down,
# fall back to security.ubuntu.com, which the hosted runners can reach.
if [ -f /etc/apt/apt-mirrors.txt ]; then
  sudo sed -i \
    -e 's|http://azure.archive.ubuntu.com|https://azure.archive.ubuntu.com|g' \
    /etc/apt/apt-mirrors.txt
fi

packages=(
  libgtk-4-dev
  libwebkitgtk-6.0-dev
  libjavascriptcoregtk-6.0-dev
  pkg-config
  "$@"
)

apt_options=(
  -o Acquire::Retries=5
  -o Acquire::ForceIPv4=true
  -o Acquire::http::Timeout=60
  -o Acquire::https::Timeout=60
)

apt_update() {
  sudo timeout --kill-after=30 300 apt-get "${apt_options[@]}" update
}

update_log=$(mktemp)
update_ok=1
if ! apt_update 2>&1 | tee "$update_log"; then
  update_ok=0
fi
if grep -qE '^(Err:|E:|W: Failed to fetch)' "$update_log"; then
  update_ok=0
fi
if [ "$update_ok" -eq 0 ]; then
  echo "Azure apt mirror is unusable; falling back to security.ubuntu.com." >&2
  if [ -f /etc/apt/apt-mirrors.txt ]; then
    sudo sed -i \
      -e 's|https://azure.archive.ubuntu.com|https://security.ubuntu.com|g' \
      /etc/apt/apt-mirrors.txt
  fi
  apt_update
fi
rm -f "$update_log"

sudo timeout --kill-after=30 2400 apt-get "${apt_options[@]}" install -y --no-install-recommends "${packages[@]}"
