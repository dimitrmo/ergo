#!/bin/sh
# nginx serves the UI and proxies the API; ergo is the main process, so it
# gets the stop signal and shuts down cleanly. If either dies, /health (which
# goes through nginx to ergo) fails and the container healthcheck restarts us.
set -e

# The sidebar icon is a nicety: never let it stop the add-on starting.
/usr/bin/sidebar-icon.sh || echo "sidebar icon: setup failed, see DOCS"

nginx -g 'daemon off;' &

exec /usr/bin/ergo serve
