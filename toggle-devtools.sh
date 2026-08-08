#!/bin/bash

FILE="main-dist/main.js"
FILE2="main-dist/compact-app.js"

if grep -q 'devTools: !1' "$FILE" "$FILE2" 2>/dev/null; then
    # Enable
    sed -i 's/devTools: !1/devTools: !0/g' "$FILE" "$FILE2"
    sed -i 's/createWithMultiWindow(i, o, yn, oe(), t)/createWithMultiWindow(i, o, yn, true, t)/g' "$FILE" "$FILE2"
    sed -i 's/oe() && r\.unshift({/r.unshift({/g' "$FILE" "$FILE2"
    echo "[+] DevTools ENABLED"
else
    # Disable
    sed -i 's/devTools: !0/devTools: !1/g' "$FILE" "$FILE2"
    sed -i 's/createWithMultiWindow(i, o, yn, true, t)/createWithMultiWindow(i, o, yn, oe(), t)/g' "$FILE" "$FILE2"
    sed -i 's/r\.unshift({$/oe() \&\& r.unshift({/g' "$FILE" "$FILE2"
    echo "[-] DevTools DISABLED"
fi
