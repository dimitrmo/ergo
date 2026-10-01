#!/bin/sh
# nginx serves the UI and proxies the API; ergo is the main process, so it
# gets the stop signal and shuts down cleanly. If either dies, /health (which
# goes through nginx to ergo) fails and the container healthcheck restarts us.
set -e

nginx -g 'daemon off;' &

exec /usr/bin/ergo serve
