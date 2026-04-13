#!/usr/bin/env bash
# check-unnamed-tuple-carriers.sh
#
# Detects Rust code that uses unnamed tuples with 4+ elements as data carriers.
# Tuples with 4+ elements are positional-confusion hazards: callers can swap
# fields without a compile error or visible signal. Named structs prevent this.
#
# Exemptions:
#   - Inline: add "// tuple-carrier:ok -- <reason>" on the declaration line.
#   - File-based: add "file:line" to scripts/unnamed-tuple-carrier-exemptions.txt
#
# Origin: TASK-048 F1 — target_artifact was set to source_artifact because a
# 5-element tuple had no named fields, making positional confusion invisible.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/unnamed-tuple-carrier-exemptions.txt"
ERRORS=0

# Load exemptions (file:line format, comments and blanks skipped)
declare -A EXEMPTED
if [ -f "$EXEMPTIONS_FILE" ]; then
    while IFS= read -r line; do
        [[ -z "$line" || "$line" =~ ^# ]] && continue
        EXEMPTED["$line"]=1
    done < "$EXEMPTIONS_FILE"
fi

# Scan all non-test Rust source files with perl for multi-line tuple detection
ALL_HITS=$(find crates/ -name '*.rs' -not -path '*/target/*' -not -path '*/tests/*' 2>/dev/null | while IFS= read -r file; do
    perl -0777 -ne '
        my $filename = "'"$file"'";
        my $content = $_;
        $content =~ s{/\*.*?\*/}{}gs;
        my $full = $content;
        my @lines = split /\n/, $full;

        # Build a set of line numbers inside #[cfg(test)] mod blocks
        my %test_lines;
        for my $i (0..$#lines) {
            if ($lines[$i] =~ /#\[cfg\(test\)\]/) {
                # Check if the NEXT non-empty, non-attribute line starts a mod block
                for my $j ($i+1..$#lines) {
                    next if $lines[$j] =~ /^\s*$/ || $lines[$j] =~ /^\s*#\[/;
                    if ($lines[$j] =~ /^\s*(pub\s+)?mod\s+/) {
                        # Mark all lines from $i to the end of this module as test lines.
                        # Use brace depth to find the module end.
                        my $depth = 0;
                        my $started = 0;
                        for my $k ($j..$#lines) {
                            for my $ch (split //, $lines[$k]) {
                                if ($ch eq "{") { $depth++; $started = 1; }
                                elsif ($ch eq "}") { $depth--; }
                            }
                            $test_lines{$k + 1} = 1;  # 1-indexed
                            last if $started && $depth <= 0;
                        }
                    }
                    last;
                }
            }
        }

        # Find Vec<( with multi-line tuple types
        while ($full =~ /\bVec<\(\s*\n?((?:[^()]*(?:\([^()]*\))?)*?)\)>/gs) {
            my $tuple_content = $1;
            my $match_pos = $-[0];
            my $prefix = substr($full, 0, $match_pos);
            my $line_num = ($prefix =~ tr/\n//) + 1;

            my $decl_line = $lines[$line_num - 1] // "";
            next if $decl_line =~ /tuple-carrier:ok/;
            next if $test_lines{$line_num};

            # Count top-level commas
            my $depth = 0;
            my $commas = 0;
            for my $ch (split //, $tuple_content) {
                if ($ch eq "(") { $depth++; }
                elsif ($ch eq ")") { $depth--; }
                elsif ($ch eq "," && $depth == 0) { $commas++; }
            }
            my $arity = $commas + 1;

            if ($arity >= 4) {
                print "VEC:$filename:$line_num:$arity\n";
            }
        }

        # Find -> (T, T, T, T) return types
        while ($full =~ /->.*?\(([^()]+(?:\([^()]*\)[^()]*)*)\)/g) {
            my $tuple_content = $1;
            my $match_pos = $-[0];
            my $prefix = substr($full, 0, $match_pos);
            my $line_num = ($prefix =~ tr/\n//) + 1;

            my $decl_line = $lines[$line_num - 1] // "";
            next if $decl_line =~ /tuple-carrier:ok/;
            next if $test_lines{$line_num};

            my $depth = 0;
            my $commas = 0;
            for my $ch (split //, $tuple_content) {
                if ($ch eq "(") { $depth++; }
                elsif ($ch eq ")") { $depth--; }
                elsif ($ch eq "<") { $depth++; }
                elsif ($ch eq ">") { $depth--; }
                elsif ($ch eq "," && $depth == 0) { $commas++; }
            }
            my $arity = $commas + 1;

            if ($arity >= 4) {
                print "RET:$filename:$line_num:$arity\n";
            }
        }
    ' "$file"
done)

# Process hits
while IFS= read -r hit; do
    [ -z "$hit" ] && continue

    kind="${hit%%:*}"
    rest="${hit#*:}"
    filepath="${rest%%:*}"
    rest2="${rest#*:}"
    linenum="${rest2%%:*}"
    arity="${rest2#*:}"

    key="$filepath:$linenum"

    # Check exemption
    if [ -n "${EXEMPTED[$key]+x}" ]; then
        continue
    fi

    if [ "$kind" = "VEC" ]; then
        echo "ERROR: $filepath:$linenum: Unnamed tuple with $arity elements used as data carrier (Vec<(...)>)."
    else
        echo "ERROR: $filepath:$linenum: Unnamed tuple with $arity elements in return type."
    fi
    echo "       Use a named struct instead to prevent positional field confusion."
    echo "       Exempt with: // tuple-carrier:ok -- <reason>"
    echo
    ERRORS=$((ERRORS + 1))
done <<< "$ALL_HITS"

if [ $ERRORS -gt 0 ]; then
    echo "Found $ERRORS unnamed tuple carrier(s) with 4+ elements."
    echo "Replace with named structs to prevent positional field confusion."
    echo "See: TASK-048 F1 — target_artifact was set to source_artifact due to tuple position confusion."
    exit 1
fi

exit 0
