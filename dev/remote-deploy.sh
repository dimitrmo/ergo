#!/bin/sh
# Runs on the Home Assistant host (via `make deploy`) after the add-on files
# are copied to /addons/ergo. Installs the local add-on the first time, then
# updates or rebuilds it. Newer HA CLIs call add-ons "apps"; both are tried.
set -e

ha store reload >/dev/null

for cmd in apps addons; do
    ha "$cmd" --help >/dev/null 2>&1 || continue
    if ha "$cmd" info local_ergo >/dev/null 2>&1 && ha "$cmd" info local_ergo | grep -q '^version: [^n]'; then
        # A changed version is an update; the same version needs a rebuild.
        ha "$cmd" update local_ergo 2>/dev/null || ha "$cmd" rebuild local_ergo
        ha "$cmd" start local_ergo 2>/dev/null || true
        echo "ergo updated"
    else
        ha "$cmd" install local_ergo
        ha "$cmd" start local_ergo
        echo "ergo installed and started"
    fi
    ha "$cmd" info local_ergo | grep -E '^(state|version|version_latest):'
    exit 0
done

echo "no 'ha apps' or 'ha addons' command found" >&2
exit 1
