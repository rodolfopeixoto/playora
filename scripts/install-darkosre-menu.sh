#!/bin/sh
echo "install-darkosre-menu.sh is deprecated."
echo "Use install-to-sd.sh — it writes the menu entries directly to the SD."
exec sh "$(dirname "$0")/install-to-sd.sh" "$@"
