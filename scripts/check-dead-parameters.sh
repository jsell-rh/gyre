#!/usr/bin/env bash
# check-dead-parameters.sh — Detect behaviorally-dead parameters in Rust.
#
# Procedural background (specs/reviews/task-072.md F6 / task-095.md R2-1):
# a decision-input parameter (sha, id, path, mode...) is threaded through a
# call chain but never used to make a decision — the function compiles, tests
# pass (against fakes that also ignore the parameter), and the specced
# behavior the parameter names silently does not happen. Observed twice:
#   - task-095 R2-1: `run_post_merge_gates(head_sha)` accepted the sha but
#     never used it to select/prepare a checkout — gates validated the wrong
#     tree while every test went green.
#   - task-072 F6: Pass 2 lookup keyed on a qualified-name format produced by
#     a different producer than the node store (binary import-path vs
#     extractor package-clause) — exact-match resolution that can only
#     succeed when producer and consumer formats coincide.
#
# Mechanically decidable slice flagged here: a decision-input parameter whose
# only references in the function body are inside non-interpolating string
# literals (log lines, doc-ish messages). A format string that interpolates
# the parameter (e.g. format!("/repos/{repo_id}/...")) counts as real use.
#
# The rest of the flaw class (parameter used in logic that selects the wrong
# thing, or format-coincidence between producer and consumer) is NOT
# mechanically decidable — it is covered by the implementation prompt's
# checklist item on decision-input parameter tracing and cross-producer
# format verification, which cites these reviews.
#
# Exempt a finding with: `// param:ok — <reason>` on the flagged fn line.
#
# Usage: bash scripts/check-dead-parameters.sh [paths...]   (default: crates/)

set -uo pipefail

if [ $# -eq 0 ]; then
    set -- crates/
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

python3 - "$@" <<'PYEOF'
import re
import sys
from pathlib import Path

# Decision-input names: the identifiers that, when accepted as parameters,
# promise the function's behavior depends on them. Suffix-based to catch
# head_sha, repo_id, root_path, etc.
PARAM_RE = re.compile(
    r'^(?:[a-z_][a-z0-9_]*_)?(?:sha|id|path|dir|cwd|workdir|root|mode|kind|filter|scope)$'
)

# fn signature start: optional pub/async/unsafe/const, fn name, open paren.
FN_RE = re.compile(r'^\s*(?:pub(?:\([a-z_]+\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+"[^"]*"\s+)?fn\s+[a-zA-Z_0-9]+\s*[<(]')

def find_files(paths):
    for p in paths:
        path = Path(p)
        if path.is_dir():
            yield from sorted(path.rglob('*.rs'))
        elif path.suffix == '.rs':
            yield path

def tokenize(text):
    """Yield (kind, text) segments: kind in {'code', 'string', 'comment'}.
    String-aware: `//` inside a string literal is not a comment, braces
    inside a literal are not code braces. Handles r"..."/r#"..."# and
    escapes. Line-oriented (no multi-line literals except raw ones are
    common in this pattern; multi-line /* */ comments are handled at the
    whole-body level by strip_code)."""
    out = []
    i = 0
    n = len(text)
    buf = []
    def flush_code():
        if buf:
            out.append(('code', ''.join(buf)))
            buf.clear()
    while i < n:
        ch = text[i]
        if ch == 'r' and i + 1 < n and text[i + 1] in '"#':
            j = i + 1
            hashes = 0
            while j < n and text[j] == '#':
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                end_marker = '"' + '#' * hashes
                j += 1
                while j < n and text[j:j + 1 + hashes] != end_marker:
                    j += 1
                flush_code()
                out.append(('string', text[i:min(j + 1 + hashes, n)]))
                i = j + 1 + hashes
                continue
        if ch == '"':
            j = i + 1
            while j < n:
                if text[j] == '\\':
                    j += 2
                    continue
                if text[j] == '"':
                    break
                j += 1
            flush_code()
            out.append(('string', text[i:min(j + 1, n)]))
            i = j + 1
            continue
        if ch == '/' and i + 1 < n and text[i + 1] == '/':
            flush_code()
            j = text.find('\n', i)
            # End at newline, NOT end-of-text: strip_code feeds the whole
            # multi-line function body; ending the comment at `n` swallowed
            # everything after the first line comment in the body.
            j = n if j == -1 else j
            out.append(('comment', text[i:j]))
            i = j
            continue
        if ch == '/' and i + 1 < n and text[i + 1] == '*':
            j = text.find('*/', i + 2)
            j = n if j == -1 else j + 2
            flush_code()
            out.append(('comment', text[i:j]))
            i = j
            continue
        buf.append(ch)
        i += 1
    flush_code()
    return out

def function_spans(lines):
    """Yield (sig_start_idx, sig_end_idx, body_end_idx, params_text) for each
    fn item with a real body (trait-method declarations are skipped)."""
    i = 0
    n = len(lines)
    while i < n:
        if FN_RE.match(lines[i]):
            # Capture until the signature's opening '{' at paren-depth 0.
            # A ';' at paren-depth 0 first means this is a trait-method
            # declaration (no body) — skip it: the "body" would run into
            # the next method's text and produce false positives.
            depth = 0
            sig_end = None
            j = i
            while j < n and j < i + 30:
                for kind, text in tokenize(lines[j]):
                    if kind != 'code':
                        continue
                    for ch in text:
                        if ch == '(':
                            depth += 1
                        elif ch == ')':
                            depth -= 1
                        elif ch == ';' and depth == 0:
                            sig_end = 'trait'
                            break
                        elif ch == '{' and depth == 0:
                            sig_end = j
                            break
                    if sig_end is not None:
                        break
                if sig_end is not None:
                    break
                j += 1
            if sig_end == 'trait' or sig_end is None:
                i += 1
                continue
            sig = ''.join(lines[i:sig_end + 1])
            # Body end: brace matching from sig_end, skipping string
            # literals and comments (braces inside them — e.g.
            # contains("table! {") — would otherwise corrupt the count).
            depth = 0
            body_end = sig_end
            k = sig_end
            while k < n and k < i + 2000:
                for kind, text in tokenize(lines[k]):
                    if kind == 'code':
                        depth += text.count('{') - text.count('}')
                if depth == 0 and k > sig_end:
                    body_end = k
                    break
                body_end = k
                k += 1
            # Param list: inside the outermost parens.
            m = re.search(r'fn\s+[a-zA-Z_0-9]+\s*(?:<[^>]*>)?\s*\(', sig)
            if m:
                params = sig[m.end():sig.rfind(')')]
                yield (i, sig_end, body_end, params)
            i = body_end + 1
        else:
            i += 1

LOG_MACRO_RE = re.compile(
    r'\b(?:tracing|log)::(?:info|warn|error|debug|trace|emit)\s*!|'
    r'\beprintln!|\beprint!|'
    r'\bprintln!\s*\('
)

def strip_code(body, param):
    """Remove comments and non-interpolating string literals. A literal that
    interpolates the parameter (format!("/x/{param}")) becomes a USEOF
    marker — building a request path or query with the parameter is real
    consumption; a log line merely mentioning the name is not.
    Log macros (tracing::*, log::*, println!/eprintln!) are dropped whole:
    the task-095 R2-1 flaw was a parameter whose only role was a log line
    ('running post-merge gates on {head_sha}') while the sha never selected
    the tree the gates ran against. A parameter used only for logging is
    behaviorally dead."""
    out = []
    i = 0
    n = len(body)
    while i < n:
        m = LOG_MACRO_RE.search(body, i)
        if m and m.start() == i:
            # Skip macro name; now skip its balanced parens entirely.
            j = body.find('(', i)
            if j == -1:
                break
            depth = 0
            k = j
            while k < n:
                ch = body[k]
                if ch == '"':
                    # skip string inside macro args
                    k += 1
                    while k < n:
                        if body[k] == '\\':
                            k += 2
                            continue
                        if body[k] == '"':
                            break
                        k += 1
                elif ch == '(':
                    depth += 1
                elif ch == ')':
                    depth -= 1
                    if depth == 0:
                        break
                k += 1
            i = k + 1
            continue
        if m:
            # emit text before the next log macro, then loop
            out.append(body[i:m.start()])
            i = m.start()
            continue
        out.append(body[i:])
        break
    prefix = ''.join(out)
    # Tokenize the remainder for strings/comments.
    out2 = []
    for kind, text in tokenize(prefix):
        if kind == 'code':
            out2.append(text)
        elif kind == 'string':
            if re.search(r'\{\s*' + re.escape(param) + r'\s*(:[^}]*)?\}', text):
                out2.append('USEOF')
    return ''.join(out2)

def main():
    errors = 0
    for path in find_files(sys.argv[1:]):
        try:
            lines = path.read_text(encoding='utf-8', errors='replace').splitlines()
        except OSError:
            continue
        for sig_start, sig_end, body_end, params in function_spans(lines):
            fn_line = lines[sig_start]
            if '// param:ok' in fn_line:
                continue
            # Body: from AFTER the signature's opening brace line, so a
            # multi-line signature's parameter declarations are not mistaken
            # for uses of the parameter.
            body = '\n'.join(lines[sig_end + 1:body_end + 1])
            for raw in params.split(','):
                p = raw.strip()
                # Remove attributes, mut, references, type.
                p = re.sub(r'^.*?\bmut\s+', '', p)
                m = re.match(r'^([a-z_][a-z0-9_]*)\s*:', p)
                if not m:
                    continue
                name = m.group(1)
                if name.startswith('_'):
                    continue  # declared intentionally unused
                if not PARAM_RE.match(name):
                    continue
                stripped = strip_code(body, name)
                if not re.search(r'\b' + re.escape(name) + r'\b|USEOF', stripped):
                    print(f"ERROR: Log-only parameter at {path}:{sig_start + 1}")
                    print(f"  Parameter `{name}` of the fn starting at line {sig_start + 1} is")
                    print("  referenced only inside string literals (log/format messages that do")
                    print("  not even interpolate it). It never selects, filters, routes, or")
                    print("  otherwise decides anything — it is behaviorally dead.")
                    print("  Flaw class: specs/reviews/task-072.md F6 / task-095.md R2-1")
                    print("  (parameter plumbed but the behavior it names does not happen).")
                    print("  Consume the parameter in real logic, or exempt the fn line with:")
                    print("  // param:ok — <reason>")
                    print()
                    errors += 1
    if errors:
        print(f"check-dead-parameters: FAILED — {errors} log-only parameter(s) found")
        sys.exit(1)
    print("check-dead-parameters: OK")

main()
PYEOF
