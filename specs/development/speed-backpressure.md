# The Wheel: Speed & Backpressure

Two dimensions matter:
1. **Speed of the wheel** - how fast code generation works.
2. **How it spins** - backpressure mechanisms (tests, lints, hooks) that keep quality without blocking speed.

Eventually, **agent-to-agent coordination** should provide much of this backpressure naturally. For now, **pre-commit hooks** are the practical mechanism.

## Pre-Commit Hooks

Example (using `prek`, a pre-commit compatible hook runner):

```yaml
default_stages: [pre-commit]

repos:
  - repo: local
    hooks:
      - id: block-backup-files        # Prevent accidental commits
      - id: block-secrets-backup      # Catch leaked secrets
      - id: cargo2nix-update           # Keep Cargo.nix in sync
      - id: i18n-coverage-web          # Enforce i18n completeness (web)
      - id: i18n-coverage-rust         # Enforce i18n completeness (rust)
      - id: gyre-web-build             # Verify web builds
      - id: shellcheck                 # Lint shell scripts
      - id: emdash-check               # Replace em-dashes with hyphens
```

These hooks mechanically enforce invariants at commit time - blocking bad patterns before they enter the repo, keeping the wheel spinning clean.
