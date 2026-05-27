set shell := ["fish", "-c"]

PODMAN_IMAGE := "cryostorage-host-build:aarch64"

default:
  @just --list

[doc("Cross build and release by copying to drive")]
@full: podman-build podman-release copy_to_switch_drive

[confirm("Copy to drive? (y/N)")]
@copy_to_switch_drive:
  cp controller/host/target/aarch64-unknown-linux-gnu/release/host ~/switchDrive/Institution/cryostorage_release/cryostorage_host

@podman-release:
  podman run --rm -it \
    --userns keep-id \
    -v "$PWD":/work:Z \
    -v cryostorage-cargo-registry:/usr/local/cargo/registry:Z \
    -v cryostorage-cargo-git:/usr/local/cargo/git:Z \
    -v cryostorage-host-target-aarch64:/work/controller/host/target:Z \
    -w /work/controller/host \
    {{PODMAN_IMAGE}} \
    cargo build --release --target aarch64-unknown-linux-gnu

@podman-build:
  podman build \
    -f controller/host/Dockerfile.aarch64-unknown-linux-gnu \
    -t {{PODMAN_IMAGE}} \
    .


