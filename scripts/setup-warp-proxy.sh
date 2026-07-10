#!/usr/bin/env bash

set -euo pipefail

if (( EUID != 0 )); then
    echo "run this script as root on the remote host" >&2
    exit 1
fi

install_apt_package() {
    apt-get update
    apt-get install --assume-yes ca-certificates curl gnupg

    install -d -m 0755 /usr/share/keyrings
    curl -fsSL https://pkg.cloudflareclient.com/pubkey.gpg |
        gpg --yes --dearmor --output /usr/share/keyrings/cloudflare-warp-archive-keyring.gpg

    # shellcheck disable=SC1091
    source /etc/os-release
    local codename="${VERSION_CODENAME:-}"

    if [[ -z "$codename" ]]; then
        echo "VERSION_CODENAME is missing from /etc/os-release" >&2
        exit 1
    fi

    printf '%s\n' \
        "deb [signed-by=/usr/share/keyrings/cloudflare-warp-archive-keyring.gpg] https://pkg.cloudflareclient.com/ $codename main" \
        > /etc/apt/sources.list.d/cloudflare-client.list

    apt-get update
    apt-get install --assume-yes cloudflare-warp
}


install_rpm_package() {
    local package_manager="$1"

    "$package_manager" install --assumeyes curl

    # shellcheck disable=SC1091
    source /etc/os-release

    case "${ID:-}" in
        rhel | centos | rocky | almalinux)
            "$package_manager" install --assumeyes epel-release
            ;;
    esac

    rpm --import https://pkg.cloudflareclient.com/pubkey.gpg
    curl -fsSL \
        https://pkg.cloudflareclient.com/cloudflare-warp-ascii.repo \
        --output /etc/yum.repos.d/cloudflare-warp.repo
    "$package_manager" makecache
    "$package_manager" install --assumeyes cloudflare-warp
}


install_cloudflare_warp() {
    if command -v apt-get >/dev/null 2>&1; then
        install_apt_package
    elif command -v dnf >/dev/null 2>&1; then
        install_rpm_package dnf
    elif command -v yum >/dev/null 2>&1; then
        install_rpm_package yum
    else
        echo "unsupported package manager; install cloudflare-warp manually" >&2
        exit 1
    fi
}


install_cloudflare_warp

readonly warp_cli="$(command -v warp-cli)"

systemctl enable --now warp-svc.service

if ! "$warp_cli" registration show >/dev/null 2>&1; then
    echo "Cloudflare WARP requires a one-time consumer registration."
    echo "The next command may ask you to accept Cloudflare's terms."
    "$warp_cli" registration new
fi

echo "WARP consumer registration is ready"
