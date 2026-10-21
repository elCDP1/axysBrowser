#!/usr/bin/env bash
set -euo pipefail

# Ubuntu 24.04 confines unprivileged user namespaces with AppArmor. WebKit's
# sandbox needs them and also configures loopback inside a new network
# namespace; when that is denied, web processes die and the UI aborts. Prefer
# Ubuntu's targeted bwrap profile and only relax the sysctl on this disposable
# runner if the profile is not enough.
probe() {
    bwrap --unshare-user --unshare-pid --unshare-net --ro-bind / / true
}

if ! probe >/dev/null 2>&1; then
    if [ -r /etc/os-release ] && . /etc/os-release && [ "${ID:-}" = "ubuntu" ]; then
        sudo apt-get install -y apparmor-profiles apparmor-utils bubblewrap

        profile=$(dpkg -L apparmor-profiles 2>/dev/null \
            | grep '/bwrap-userns-restrict$' \
            | head -n1 || true)

        if [ -n "$profile" ]; then
            sudo install -m 0644 "$profile" /etc/apparmor.d/bwrap-userns-restrict
            sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict || true
        fi
    fi
fi

if ! probe; then
    echo "bwrap still cannot set up namespaces; relaxing the AppArmor userns restriction" >&2
    sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
    sudo sysctl -w kernel.unprivileged_userns_clone=1 || true
    probe
fi

echo "WebKit sandbox namespaces are ready"
