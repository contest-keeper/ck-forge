#!/bin/bash
# Runs once inside the Ubuntu cloud image, started by cloud-init as the user-data script.
# It turns the minimal server into a contest desktop, then powers the machine off. The program
# that started the VM looks for the marker lines on the serial port.
exec > >(tee -a /dev/ttyS0) 2>&1

fetch_pinned() {
  local url=$1 sha256=$2 out=$3
  curl --fail --silent --show-error --location --output "$out" "$url"
  printf '%s  %s\n' "$sha256" "$out" | sha256sum --check --status
}

provision() {
  set -euxo pipefail
  export DEBIAN_FRONTEND=noninteractive

  apt-get update
  # The Keeper patch uses Squid with TLS support, nftables and FFmpeg.
  apt-get install -y --no-install-recommends \
    xubuntu-core lightdm lightdm-gtk-greeter \
    build-essential gdb python3 python3-pip openjdk-21-jdk \
    codeblocks geany vim nano \
    squid-openssl nftables ffmpeg libnotify-bin network-manager \
    curl ca-certificates software-properties-common unzip

  # Ubuntu's Firefox is a snap, which needs the snap store; the Mozilla team's deb does not.
  add-apt-repository -y ppa:mozillateam/ppa
  printf 'Package: *\nPin: release o=LP-PPA-mozillateam\nPin-Priority: 1001\n' \
    >/etc/apt/preferences.d/mozilla-firefox
  apt-get install -y firefox

  # Match the pinned releases used in the Keeper OS image. Ubuntu has no equivalent packages.
  downloads=$(mktemp -d)
  trap 'rm -rf -- "${downloads:?}"' EXIT
  fetch_pinned \
    'https://github.com/VSCodium/vscodium/releases/download/1.143.17232/codium_1.143.17232_amd64.deb' \
    '988330df203f4d2dbfb9ee0052a0356a27af7c0a2a7ff606424c531ae4a7b538' \
    "$downloads/codium.deb"
  apt-get install -y --no-install-recommends "$downloads/codium.deb"
  fetch_pinned \
    'https://github.com/JetBrains/kotlin/releases/download/v2.4.21/kotlin-compiler-2.4.21.zip' \
    '7cc140e76daf416a0424a5557f99afd7ca89ebd463ea1101261e3e01c33e75cd' \
    "$downloads/kotlin.zip"
  unzip -q "$downloads/kotlin.zip" -d /opt
  for tool in kotlinc kotlin kotlinc-jvm; do
    ln -sfn "/opt/kotlinc/bin/$tool" "/usr/local/bin/$tool"
  done
  mkdir -p /etc/skel/.config/VSCodium/User
  printf '{"chat.disableAIFeatures":true}\n' >/etc/skel/.config/VSCodium/User/settings.json

  # A contest machine must not change under the contestants.
  systemctl disable --now unattended-upgrades apt-daily.timer apt-daily-upgrade.timer || true
  apt-get purge -y unattended-upgrades || true

  # LightDM starts the local session without enabling password sign-in.
  id contestant >/dev/null 2>&1 || useradd -m -s /bin/bash contestant
  passwd -l contestant
  mkdir -p /etc/lightdm/lightdm.conf.d
  printf '[Seat:*]\nautologin-user=contestant\nautologin-user-timeout=0\nautologin-session=xfce\nallow-guest=false\ngreeter-hide-users=true\n' \
    >/etc/lightdm/lightdm.conf.d/50-keeper.conf
  systemctl set-default graphical.target

  # Leave a small image: no package caches, no machine-specific state.
  apt-get autoremove -y
  apt-get clean
  rm -rf /var/lib/apt/lists/*
  cloud-init clean --logs --seed
  # Without this the stick would wait for a cloud datasource at every boot.
  touch /etc/cloud/cloud-init.disabled
  truncate -s 0 /etc/machine-id
  fstrim -av || true
}

# Testing the child directly in an `if` would disable Bash's errexit inside it.
set +e
(provision)
status=$?
if [[ $status -eq 0 ]]; then
  echo "CK-PROVISION-OK"
else
  echo "CK-PROVISION-FAILED"
fi
sync
poweroff
