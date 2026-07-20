#!/usr/bin/env bash
set -euo pipefail

# Installs and configures a pinned OpenRCT2 release for local development.
#
# Facts below verified against github.com/OpenRCT2/OpenRCT2 tag v0.5.3
# (commit f503f57bdb74b31507f83909db587a5db5794ef0). Full citation list in
# docs/OPENRCT2_INTEGRATION.md.
#   - openrct2-cli sets headless mode unconditionally: src/openrct2-cli/Cli.cpp
#   - config.ini [general] "game_path" key: src/openrct2/config/Config.cpp:200,311
#   - default config.ini location ~/.config/OpenRCT2/config.ini:
#     src/openrct2/PlatformEnvironment.cpp:262 + src/openrct2/platform/Platform.Linux.cpp:84-99
#   - `set-rct2 <path>` validates <path>/Data/g1.dat, writes config.ini:
#     src/openrct2/command_line/RootCommands.cpp:337-390
#   - `scan-objects --verbose` prints the DirBase::rct2 path and builds the
#     object index, proving the configured path was read:
#     src/openrct2/command_line/RootCommands.cpp:392-408, src/openrct2/PlatformEnvironment.cpp:311

OPENRCT2_VERSION="v0.5.3"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GOG_APP_PATH="${REPO_ROOT}/assets/gog/extracted/app"
INSTALL_DIR="${REPO_ROOT}/runtime/openrct2/${OPENRCT2_VERSION}"
BIN="${INSTALL_DIR}/OpenRCT2/openrct2-cli"

log() { printf '[setup-openrct2] %s\n' "$*"; }
fail() { printf '[setup-openrct2] ERROR: %s\n' "$*" >&2; exit 1; }

command -v gh >/dev/null 2>&1 || fail "gh CLI is required (used to fetch the pinned GitHub release)."

if [ ! -d "${GOG_APP_PATH}/Data" ]; then
    fail "GOG assets not found at ${GOG_APP_PATH} (expected Data/, ObjData/, Tracks/, Scenarios/). See assets/gog/README.md."
fi

if [ -x "${BIN}" ]; then
    log "OpenRCT2 ${OPENRCT2_VERSION} already installed at ${INSTALL_DIR}, skipping download."
else
    # shellcheck source=/dev/null
    . /etc/os-release
    CODENAME="${VERSION_CODENAME:-}"
    [ -n "${CODENAME}" ] || fail "Could not determine distro codename from /etc/os-release."

    ASSET="OpenRCT2-${OPENRCT2_VERSION}-Linux-${CODENAME}-x86_64.tar.gz"
    SUMS="OpenRCT2-${OPENRCT2_VERSION}-sha256sums.txt"

    log "Downloading ${ASSET} (release ${OPENRCT2_VERSION})..."
    WORKDIR="$(mktemp -d)"
    trap 'rm -rf "${WORKDIR}"' EXIT

    gh release download "${OPENRCT2_VERSION}" --repo OpenRCT2/OpenRCT2 \
        --dir "${WORKDIR}" --pattern "${ASSET}" --pattern "${SUMS}" \
        || fail "No release asset '${ASSET}' for this distro (${CODENAME}). Check https://github.com/OpenRCT2/OpenRCT2/releases/tag/${OPENRCT2_VERSION} for available Linux builds and adapt CODENAME detection above."

    log "Verifying checksum..."
    (cd "${WORKDIR}" && grep -F "${ASSET}" "${SUMS}" | sha256sum -c -) \
        || fail "Checksum verification failed for ${ASSET}."

    mkdir -p "${INSTALL_DIR}"
    tar xzf "${WORKDIR}/${ASSET}" -C "${INSTALL_DIR}"
    log "Installed to ${INSTALL_DIR}."
fi

"${BIN}" --version

log "Setting game_path to ${GOG_APP_PATH} (via 'openrct2-cli set-rct2')..."
"${BIN}" set-rct2 "${GOG_APP_PATH}"

log "Proving object data loads from the configured game_path (via 'openrct2-cli scan-objects --verbose')..."
PROOF_LOG="$(mktemp)"
"${BIN}" scan-objects --verbose > "${PROOF_LOG}" 2>&1

if ! grep -F "DirBase::rct2" "${PROOF_LOG}" | grep -qF "${GOG_APP_PATH}"; then
    cat "${PROOF_LOG}" >&2
    fail "Did not find the expected DirBase::rct2 log line pointing at ${GOG_APP_PATH}."
fi
if ! grep -qE "Building object index \([0-9]+ items\)" "${PROOF_LOG}"; then
    cat "${PROOF_LOG}" >&2
    fail "Did not find the expected 'Building object index (N items)' line."
fi

grep -E "DirBase::rct2|Building object index|Finished building object index" "${PROOF_LOG}"
rm -f "${PROOF_LOG}"

log "OpenRCT2 ${OPENRCT2_VERSION} installed and configured. Binary: ${BIN}"
