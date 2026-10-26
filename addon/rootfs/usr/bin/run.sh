#!/bin/sh
# nginx serves the UI and proxies the API; ergo is the main process, so it
# gets the stop signal and shuts down cleanly. If either dies, /health (which
# goes through nginx to ergo) fails and the container healthcheck restarts us.
set -e

# The sidebar icon needs ergo-icons.js under HA's /local/. HA only serves
# /local/ if www/ existed when it started, so a new www/ needs an HA restart.
if [ -d /homeassistant ]; then
  [ -d /homeassistant/www ] || echo "created /homeassistant/www: restart Home Assistant once for the sidebar icon"
  mkdir -p /homeassistant/www
  cmp -s /usr/share/ergo/ergo-icons.js /homeassistant/www/ergo-icons.js \
    || cp /usr/share/ergo/ergo-icons.js /homeassistant/www/ergo-icons.js
fi

nginx -g 'daemon off;' &

exec /usr/bin/ergo serve
