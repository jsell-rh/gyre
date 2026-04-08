#!/usr/bin/env bash
# Architecture lint: detect Svelte components that are defined but never
# imported by any other file.
#
# When an agent creates a new UI component (e.g., ProvenanceChain.svelte)
# but never imports it into a parent component, the component is dead code.
# It compiles, tests pass (if it has its own tests), but the feature is
# invisible to users — the acceptance criterion "Explorer shows X" is not
# satisfied even though the component exists.
#
# Detection: for each .svelte file, check if its component name appears in
# any other file's import statements. A component that is never imported
# anywhere is unreferenced.
#
# Exempt with a comment in the component file: <!-- dead-component:ok -->
# (e.g., for storybook-only components or test fixtures)
#
# See: specs/reviews/task-009.md F3 (ProvenanceChain.svelte dead code)
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
VIOLATIONS=0

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping dead component check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for unreferenced Svelte components..."

# Entry points that don't need to be imported (they are route pages or the app root)
# App.svelte is the root, +page.svelte / +layout.svelte are SvelteKit routes
ENTRY_PATTERNS="App\.svelte$|\+page\.svelte$|\+layout\.svelte$|\+error\.svelte$|\+page\.server\."

for component in $(find "$WEB_SRC" -name '*.svelte' -type f | sort); do
    basename_component=$(basename "$component")
    name_no_ext="${basename_component%.svelte}"

    # Skip entry point files
    if echo "$basename_component" | grep -qE "$ENTRY_PATTERNS"; then
        continue
    fi

    # Skip files with exemption comment
    if grep -q 'dead-component:ok' "$component" 2>/dev/null; then
        continue
    fi

    # Check if any other file imports this component
    # Patterns checked:
    #   import X from '...ComponentName.svelte'
    #   import X from "...ComponentName.svelte"
    #   import { X } from '...ComponentName...'
    #   <ComponentName (used as a Svelte component tag)
    IMPORT_COUNT=$(grep -rl "$name_no_ext" "$WEB_SRC" --include='*.svelte' --include='*.ts' --include='*.js' 2>/dev/null \
        | grep -v "^${component}$" \
        | head -1 \
        | wc -l || true)

    if [ "$IMPORT_COUNT" -eq 0 ]; then
        echo ""
        echo "DEAD COMPONENT: $component"
        echo "  Component '$name_no_ext' is not imported or referenced by any other file."
        echo "  Either wire it into a parent component, or remove it."
        echo "  A component that exists but is never rendered is dead code —"
        echo "  acceptance criteria requiring the feature to be visible are NOT satisfied."
        echo ""
        echo "  Add '<!-- dead-component:ok -->' to exempt (e.g., storybook-only components)."
        echo ""
        VIOLATIONS=$((VIOLATIONS + 1))
    fi
done

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Dead component check passed."
    exit 0
else
    echo "Fix: Import dead components into parent components to wire them into the UI."
    echo "     If a component was created for a spec requirement (e.g., §7.6 visualization),"
    echo "     it must be imported and rendered — creating the file alone does not satisfy"
    echo "     the acceptance criterion."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
