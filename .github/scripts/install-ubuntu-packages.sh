#!/usr/bin/env bash
set -euo pipefail

sources=(
  -o Dir::Etc::sourcelist=/etc/apt/sources.list.d/ubuntu.sources
  -o Dir::Etc::sourceparts=-
)
sudo apt-get "${sources[@]}" update
sudo apt-get "${sources[@]}" install -y "$@"
