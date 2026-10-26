#!/bin/sh
# Installs the sidebar icon: copies ergo-icons.js to HA's www/ and loads it on
# every page through frontend: extra_module_url in configuration.yaml.
# Only an edit that is clearly safe is made; anything else is logged with what
# to add by hand. The `sidebar_icon` option turns this off.
set -eu

HA=/homeassistant
CONFIG=$HA/configuration.yaml
URL=/local/ergo-icons.js

grep -q '"sidebar_icon": *false' /data/options.json 2>/dev/null && exit 0
[ -d "$HA" ] || exit 0

# HA only serves /local/ if www/ existed when it started.
[ -d "$HA/www" ] || echo "sidebar icon: created $HA/www, restart Home Assistant"
mkdir -p "$HA/www"
cmp -s /usr/share/ergo/ergo-icons.js "$HA/www/ergo-icons.js" \
  || cp /usr/share/ergo/ergo-icons.js "$HA/www/ergo-icons.js"

[ -f "$CONFIG" ] || exit 0
grep -q 'ergo-icons\.js' "$CONFIG" && exit 0

manual() {
  echo "sidebar icon: $1; add this to configuration.yaml and restart Home Assistant:"
  echo "  frontend:"
  echo "    extra_module_url:"
  echo "      - $URL"
  exit 0
}

# An existing extra_module_url list would need merging, so leave it alone.
if grep -q '^[[:space:]]*extra_module_url:' "$CONFIG"; then
  echo "sidebar icon: add \"- $URL\" to extra_module_url in configuration.yaml and restart Home Assistant"
  exit 0
fi

frontend=$(grep -n '^frontend:' "$CONFIG" || true)
if [ -z "$frontend" ]; then
  new=$(printf '\n# Added by the ergo add-on for its sidebar icon.\nfrontend:\n  extra_module_url:\n    - %s\n' "$URL")
  cp "$CONFIG" "$CONFIG.ergo-backup"
  printf '%s\n' "$new" >> "$CONFIG"
elif [ "$(echo "$frontend" | wc -l)" -ne 1 ]; then
  manual "configuration.yaml has more than one frontend: key"
elif ! echo "$frontend" | grep -q '^[0-9]*:frontend:[[:space:]]*\(#.*\)\?$'; then
  # `frontend: !include ...` or an inline value.
  manual "frontend: isn't a plain block"
else
  line=${frontend%%:*}
  # Indent like the block's first child, or two spaces if it has none.
  indent=$(awk -v n="$line" 'NR > n && /^[[:space:]]+[^[:space:]#]/ { match($0, /^[[:space:]]+/); print substr($0, 1, RLENGTH); exit } NR > n && /^[^[:space:]#]/ { exit }' "$CONFIG")
  [ -n "$indent" ] || indent='  '
  cp "$CONFIG" "$CONFIG.ergo-backup"
  awk -v n="$line" -v i="$indent" -v url="$URL" '
    { print }
    NR == n { print i "extra_module_url:"; print i i "- " url }
  ' "$CONFIG.ergo-backup" > "$CONFIG.ergo-new"
  cat "$CONFIG.ergo-new" > "$CONFIG"
  rm "$CONFIG.ergo-new"
fi
echo "sidebar icon: added $URL to configuration.yaml (backup in configuration.yaml.ergo-backup), restart Home Assistant"
