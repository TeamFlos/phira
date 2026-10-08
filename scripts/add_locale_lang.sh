#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    echo "Usage: $0 <lang-id> <lang-name>"
    echo "  lang-id:   BCP 47 language tag, e.g. es-ES"
    echo "  lang-name: Display name in that language, e.g. Español"
    exit 1
}

[[ $# -eq 2 ]] || usage

LANG_ID="$1"
LANG_NAME="$2"

# Validate format loosely
if ! [[ "$LANG_ID" =~ ^[a-z]{2,3}-[A-Z]{2,3}$ ]]; then
    echo "Error: lang-id should look like 'en-US' or 'zh-CN'" >&2
    exit 1
fi

# Create locale dirs and empty .ftl files
for base in prpr phira; do
    ref_dir="$SCRIPT_DIR/$base/locales/en-US"
    new_dir="$SCRIPT_DIR/$base/locales/$LANG_ID"

    if [[ -d "$new_dir" ]]; then
        echo "Warning: $new_dir already exists, skipping file creation"
    else
        mkdir -p "$new_dir"
        for ftl in "$ref_dir"/*.ftl; do
            touch "$new_dir/$(basename "$ftl")"
        done
        echo "Created $new_dir with $(ls "$new_dir" | wc -l) empty .ftl files"
    fi
done

# Insert into prpr-l10n/src/lib.rs in sorted order
LIB_RS="$SCRIPT_DIR/prpr-l10n/src/lib.rs"

# Check if already present
if grep -qF "\"$LANG_ID\"" "$LIB_RS"; then
    echo "Warning: $LANG_ID already exists in $LIB_RS, skipping"
    exit 0
fi

# Build the new entry line
NEW_LINE="    \"$LANG_ID\": \"$LANG_NAME\","

# Find the langs! block and insert in sorted order using Python
python3 - "$LIB_RS" "$LANG_ID" "$NEW_LINE" <<'PYEOF'
import sys, re

path, lang_id, new_line = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path).read()

# Match the langs! { ... } block
pattern = re.compile(r'(langs!\s*\{)([^}]*?)(\})', re.DOTALL)
m = pattern.search(text)
if not m:
    print("Error: could not find langs! block in lib.rs", file=sys.stderr)
    sys.exit(1)

block_body = m.group(2)
lines = block_body.split('\n')

# Collect entry lines (non-empty, non-whitespace-only)
entry_lines = [(l, l.strip().split(':')[0].strip().strip('"')) for l in lines if l.strip()]
entry_lines.append((new_line, lang_id))
entry_lines.sort(key=lambda x: x[1])

new_body = '\n' + '\n'.join(l for l, _ in entry_lines) + '\n'
new_text = text[:m.start()] + m.group(1) + new_body + m.group(3) + text[m.end():]

open(path, 'w').write(new_text)
print(f"Inserted {lang_id} into {path}")
PYEOF
