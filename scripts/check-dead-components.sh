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
# any other file's import statements or as a Svelte component tag.
# A component that is only mentioned in comments (e.g., "// see
# PipelineOverview.svelte") is still dead code — comment references
# do NOT count as live references.
#
# See: specs/reviews/task-052.md F4 (PipelineOverview dead code passed
# the old grep-based check because it was mentioned in comments)
#
# Two additional false-negative paths were discovered:
#
# 1. IMPORT WITHOUT TAG (task-052 R2 F7): A .svelte file imports a
#    component but never renders it as a tag. The import was left behind
#    after functionality was moved inline. An import alone in a .svelte
#    file is NOT a live reference — the importing file must also contain
#    <ComponentName or svelte:component usage.
#
# 2. TEST FILE REFERENCES (task-052 R2 F8): A test file (.test.js)
#    imports a component for testing, masking the fact that no production
#    code renders it. Test files are excluded from live reference counting
#    — a component referenced only by its test file is dead code, and the
#    test validates unreachable code.
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

    # Check if any other file imports or uses this component as a tag.
    #
    # IMPORTANT: plain text matches (e.g., component name in a comment like
    # "// see PipelineOverview.svelte") do NOT count as valid references.
    # A component referenced only in comments is still dead code.
    #
    # Patterns that count as LIVE references:
    #   import X from '...ComponentName.svelte'  (ES import)
    #   import X from "...ComponentName.svelte"   (ES import)
    #   import { X } from '...ComponentName...'   (named import)
    #   <ComponentName                            (Svelte component tag)
    #   <ComponentName>                           (Svelte component tag)
    #   svelte:component this={ComponentName}     (dynamic component)
    #
    # Patterns that do NOT count:
    #   // see ComponentName.svelte               (comment)
    #   /* ComponentName is used for ... */        (comment)
    #   <!-- ComponentName -->                    (HTML comment)

    LIVE_REF_COUNT=0
    for ref_file in $(find "$WEB_SRC" -type f \( -name '*.svelte' -o -name '*.ts' -o -name '*.js' \) ! -path '*/node_modules/*' 2>/dev/null); do
        [ "$ref_file" = "$component" ] && continue

        # RULE 1: Test files do NOT count as live references.
        # A component imported only by its test file (e.g., PipelineOverview.test.js)
        # is still dead — the test validates unreachable code.
        # See: specs/reviews/task-052.md F8
        ref_basename=$(basename "$ref_file")
        if echo "$ref_basename" | grep -qE '\.(test|spec)\.(js|ts|jsx|tsx)$'; then
            continue
        fi

        # RULE 2: In .svelte files, an import ALONE is not a live reference.
        # The importing file must also render the component as a tag (<ComponentName)
        # or use it dynamically (svelte:component this={ComponentName}).
        # An import without tag usage is dead code — the component was imported
        # but the functionality was implemented inline instead of rendering it.
        # See: specs/reviews/task-052.md F7
        if echo "$ref_basename" | grep -qE '\.svelte$'; then
            has_import=0
            has_tag_or_dynamic=0

            if grep -qE "import\s.*['\"].*${name_no_ext}" "$ref_file" 2>/dev/null; then
                has_import=1
            fi

            if grep -qE "<${name_no_ext}([[:space:]/>]|$)" "$ref_file" 2>/dev/null; then
                has_tag_or_dynamic=1
            fi

            if grep -qE "svelte:component.*${name_no_ext}" "$ref_file" 2>/dev/null; then
                has_tag_or_dynamic=1
            fi

            # A .svelte file counts as a live reference only if it both imports
            # AND renders the component. Import-only = dead import.
            if [ "$has_import" -eq 1 ] && [ "$has_tag_or_dynamic" -eq 1 ]; then
                LIVE_REF_COUNT=1
                break
            fi

            # Also check for tag usage without import (e.g., global component)
            if [ "$has_tag_or_dynamic" -eq 1 ]; then
                LIVE_REF_COUNT=1
                break
            fi

            continue
        fi

        # RULE 3: In non-test .js/.ts files, an import counts as a live reference.
        # These could be router files, app entry points, dynamic renderers, etc.
        if grep -qE "import\s.*['\"].*${name_no_ext}" "$ref_file" 2>/dev/null; then
            LIVE_REF_COUNT=1
            break
        fi

        # Check for Svelte component tag usage in non-Svelte files (rare but possible)
        if grep -qE "<${name_no_ext}([[:space:]/>]|$)" "$ref_file" 2>/dev/null; then
            LIVE_REF_COUNT=1
            break
        fi

        # Check for dynamic component usage
        if grep -qE "svelte:component.*${name_no_ext}" "$ref_file" 2>/dev/null; then
            LIVE_REF_COUNT=1
            break
        fi
    done

    if [ "$LIVE_REF_COUNT" -eq 0 ]; then
        echo ""
        echo "DEAD COMPONENT: $component"
        echo "  Component '$name_no_ext' has no live references in any production file."
        echo ""
        echo "  What counts as a LIVE reference:"
        echo "    - In .svelte files: import + tag usage (<$name_no_ext> or svelte:component)"
        echo "    - In non-test .js/.ts files: an import statement"
        echo ""
        echo "  What does NOT count:"
        echo "    - References in comments (// see $name_no_ext.svelte)"
        echo "    - References in test files (*.test.js, *.spec.js)"
        echo "    - Import without tag usage in .svelte files (dead import)"
        echo ""
        echo "  A component that is imported but never rendered as a tag is dead code"
        echo "  — the import was left behind after functionality was moved inline."
        echo "  Tests for a dead component validate unreachable code."
        echo ""
        echo "  Fix: Either render <$name_no_ext> in a parent component, remove the dead"
        echo "  import, or remove the component entirely."
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
