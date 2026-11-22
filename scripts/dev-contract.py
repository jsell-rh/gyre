#!/usr/bin/env python3
"""Stable requirement generations and task-graph diagnostics (no agent claims)."""
import hashlib
import json
import re

CHECKBOX = r'(?m)^([ \t]*[-*+] \[)[ xX](\] )'


def restore_checkbox_markers(assigned, observed):
    """Restore agent-toggled markers, never accept an amended requirement.

    Return the original observation for any other protected-content change so
    the normal strict contract guard still rejects it. Operational notes and
    progress metadata are retained; the assigned markers remain authoritative.
    """
    try:
        before = requirement_parts(assigned)
        after = requirement_parts(observed)
    except ValueError:
        return observed  # Preserve the checkpoint; the strict guard diagnoses it.
    normalize = lambda text: re.sub(CHECKBOX, r'\1 \2', text)
    if before == after or before[0] != after[0] or normalize(before[1]) != normalize(after[1]):
        return observed
    markers = iter(m.group(0) for m in re.finditer(CHECKBOX, before[1]))
    parts = observed.split('---', 2)
    chunks = re.split(r'(?m)(?=^## )', parts[2])
    operational = ('Shipped','Implementation Notes','Implementation Log','Review')
    if re.search(r'^title:\s*"Repair verified failure on main [0-9a-f]{12}"$',before[0],re.M):
        operational += ('Baseline failure',)
    for i, chunk in enumerate(chunks):
        if any(re.match(r'^## '+re.escape(name)+r'\s*\n',chunk) for name in operational):continue
        chunks[i] = re.sub(CHECKBOX, lambda _m: next(markers), chunk)
    parts[2] = ''.join(chunks)
    restored = '---'.join(parts)
    if requirement_parts(restored) != before:
        return observed
    return restored


def requirement_parts(body, *, include_baseline_diagnostics=False):
    parts = body.split("---", 2)
    if len(parts) != 3:
        raise ValueError("task has no YAML frontmatter")
    blocks = re.split(r"^(?=[a-z_][\w-]*:)", parts[1], flags=re.M)
    front = "\n".join(block.strip() for block in blocks
                      if block.split(":", 1)[0] in ("title", "spec_ref", "depends_on", "coverage_sections"))
    operational = ['Shipped', 'Implementation Notes', 'Implementation Log', 'Review']
    # Generated prerequisite logs are observations, not desired requirements.
    # Keep the base/fingerprint and Required behavior in the contract.
    if not include_baseline_diagnostics and re.search(
            r'^title:\s*"Repair verified failure on main [0-9a-f]{12}"$', front, re.M):
        operational.append('Baseline failure')
    prose = re.sub(r"^## (?:" + '|'.join(operational) + r")\s*\n.*?(?=^## |\Z)",
                   "", parts[2], flags=re.M | re.S)
    # Operational sections can leave surrounding blank lines behind. Match
    # the same canonical whitespace used by the generation hash so appending
    # Shipped or Review notes does not masquerade as a contract amendment.
    return front.strip(), prose.strip()


def generation(body, specs, goal="", *, include_baseline_diagnostics=False):
    front, prose = requirement_parts(body, include_baseline_diagnostics=include_baseline_diagnostics)
    refs = set(re.findall(r"[\w-]+\.md", front))
    requirements = {path: text for path, text in specs.items() if path.rsplit("/", 1)[-1] in refs}
    missing = sorted(ref for ref in refs if not any(path.endswith("/" + ref) for path in requirements)
                     and not ref.startswith("task-"))
    payload = {"task": front.strip() + "\n" + prose.strip(), "specs": requirements,
               "missing": missing, "goal": goal}
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()


def graph_errors(graph):
    errors = {name: "Missing dependencies: " + ", ".join(sorted(set(deps) - graph.keys()))
              for name, deps in graph.items() if set(deps) - graph.keys()}
    visiting, done, path = set(), set(), []
    def visit(name):
        if name in visiting:
            cycle = path[path.index(name):] + [name]
            for member in cycle:
                errors[member] = "Dependency cycle: " + " -> ".join(cycle)
            return
        if name in done or name not in graph:
            return
        visiting.add(name); path.append(name)
        for dep in graph[name]:
            visit(dep)
        path.pop(); visiting.remove(name); done.add(name)
    for name in graph:
        visit(name)
    return errors
