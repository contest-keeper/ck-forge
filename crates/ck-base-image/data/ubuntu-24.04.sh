#!/bin/bash
# Runs once inside the Ubuntu cloud image, started by cloud-init as the user-data script.
# It turns the minimal server into a contest desktop, then powers the machine off. The program
# that started the VM looks for the marker lines on the serial port.
exec > >(tee -a /dev/ttyS0) 2>&1

provision() {
  set -euxo pipefail
  export DEBIAN_FRONTEND=noninteractive

  apt-get update
  # A lightweight desktop and the tools a contest needs. squid is required by the Keeper network lock.
  apt-get install -y --no-install-recommends \
    xubuntu-core lightdm lightdm-gtk-greeter \
    build-essential gdb python3 python3-pip openjdk-21-jdk \
    geany vim nano \
    squid curl ca-certificates software-properties-common

  # Ubuntu's Firefox is a snap, which needs the snap store; the Mozilla team's deb does not.
  add-apt-repository -y ppa:mozillateam/ppa
  printf 'Package: *\nPin: release o=LP-PPA-mozillateam\nPin-Priority: 1001\n' \
    >/etc/apt/preferences.d/mozilla-firefox
  apt-get install -y firefox

  # A contest machine must not change under the contestants.
  systemctl disable --now unattended-upgrades apt-daily.timer apt-daily-upgrade.timer || true
  apt-get purge -y unattended-upgrades || true

  # Contestants log in as this user; the account has no password and cannot be used over the network.
  id contestant >/dev/null 2>&1 || useradd -m -s /bin/bash contestant
  passwd -d contestant
  mkdir -p /etc/lightdm/lightdm.conf.d
  printf '[Seat:*]\nautologin-user=contestant\nautologin-user-timeout=0\n' \
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

if (provision); then
  echo "CK-PROVISION-OK"
else
  echo "CK-PROVISION-FAILED"
fi
sync
poweroff
