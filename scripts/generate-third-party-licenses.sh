#!/bin/sh
# Regenerates the committed third-party license text file the viewer's About
# floater embeds (sl-client-bevy-viewer/assets/licenses/
# third-party-licenses.txt) from the workspace dependency graph via
# cargo-about (configuration: about.toml, template:
# scripts/third-party-licenses.hbs).
#
# Run after any dependency change; the ggh pre-commit check
# rust:::cargo-about-outdated rejects commits while the file is stale.
set -eu
cd "$(dirname "$0")/.."
cargo about generate scripts/third-party-licenses.hbs \
  --output-file sl-client-bevy-viewer/assets/licenses/third-party-licenses.txt
