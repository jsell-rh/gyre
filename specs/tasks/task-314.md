---
title: "User-facing docs as a Starlight (Astro) site deployed to GitHub Pages"
spec_ref: "Spec gap spec-17849f2036d5093f11e2a1ae / docs-site-starlight-pages / d1e5376399b8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/ci-docs-release.md §User-Facing Docs: user-facing documentation is built with Starlight (Astro) and deployed via GitHub Pages; docs are a first-class deliverable. (1) Add a real Astro + Starlight project (e.g. docs-site/ with astro.config.mjs, package.json, committed package-lock.json, following the web/ dashboard's npm conventions) whose content source is the EXISTING markdown under docs/ (api-reference.md, cli.md, server-config.md, ui.md, development.md, agent-protocol.md) — the site renders those files as its pages via Starlight content collections (frontmatter supplied by the site layer or a build step), with no forked duplicate of the content: docs/*.md stays the single source, and a page removed from docs/ disappears from the built site. (2) `npm --prefix docs-site install && npm --prefix docs-site run build` succeeds and produces dist/ output containing a rendered HTML page per docs/*.md file. (3) GitHub Pages deployment: .github/workflows/docs.yml builds the site and deploys it via actions/configure-pages + actions/upload-pages-artifact + actions/deploy-pages, triggered by changes to docs/ and docs-site/ on the default branch, and reusable via workflow_dispatch so the release publish flow (release-flow-artifacts-publish) can trigger the docs deployment for a release. (4) The site build is added as a mandatory CI step in .github/workflows/ci.yml (no `|| true` escape) so it cannot silently rot; steps follow existing workflow conventions. Do not move or rewrite the docs/ markdown content itself (other documents and code link to it), and do not alter the spec.

## Acceptance Criteria

- [ ] `npm --prefix docs-site run build` exits 0 and the built output contains one rendered HTML page per file currently in docs/ (api-reference, cli, server-config, ui, development, agent-protocol), with the markdown body content present in the rendered HTML.
- [ ] Removing (or renaming) a file under docs/ and rebuilding changes the built site's page set accordingly — the site is generated from docs/, not from a copied fork of the content.
- [ ] .github/workflows/docs.yml exists, builds the site, and deploys via actions/deploy-pages (upload-pages-artifact + deploy-pages steps present), triggering on default-branch changes to docs/ and docs-site/ and via workflow_dispatch.
- [ ] ci.yml contains a mandatory docs-site build step that fails when the site build fails.

## Verification intent

Pre-task the defect is directly observable: no astro.config.* and no astro/starlight dependency exist anywhere in the repo, and no workflow in .github/workflows/ contains a Pages deployment step — so `npm --prefix docs-site run build` fails (no such project) and a search for deploy-pages in .github/workflows/ returns nothing. Probe: build the site and assert the rendered HTML for docs/cli.md exists in the output and the Pages workflow with a deploy-pages step exists; both assertions fail on the current tree. Content-fidelity control: temporarily delete docs/cli.md, rebuild, and assert the cli page is absent from the built site — an implementation that vendors a static copy of the docs content fails this control (the page would survive the deletion).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/ci-docs-release.md`. Source: `7e74623345bd6c3cffe0d828aa1b7c8d3690c884`; plan: `a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267`.

[
  {
    "id": "cdd27aa428258fd587e513cd",
    "decision": "gap",
    "requirement_quote": "- **Starlight** (Astro) for user-facing documentation.\n- Deployed via **GitHub Pages**.\n- Docs are a first-class deliverable, not an afterthought.",
    "detail": "The spec requires user-facing documentation to be built with Starlight (Astro) and deployed via GitHub Pages. The repository's user-facing documentation is plain Markdown under docs/ (api-reference.md, cli.md, server-config.md, ui.md, etc.) served by nothing; there is no Astro/Starlight project anywhere in the repo (no astro.config.*, no astro/starlight dependency in any package manifest — the only web build is the Svelte dashboard under web/), and there is no GitHub Pages deployment: .github/workflows/ contains exactly two workflows (ci.yml and e2e.yml), neither of which deploys documentation. The markdown docs being extensive satisfies the 'first-class deliverable' spirit only partially; the two concrete named requirements (Starlight toolchain, GitHub Pages deployment) are unimplemented.",
    "evidence": [
      {
        "file": "docs/cli.md",
        "quote": "# Gyre CLI Reference",
        "detail": "User-facing documentation is plain Markdown files under docs/ (cli.md, api-reference.md, server-config.md, ui.md, development.md, agent-protocol.md). Inspected the whole repository for an Astro/Starlight docs site: no astro.config.* exists, and no package.json (root, web/, docker/gyre-agent) declares astro or @astrojs/starlight as a dependency — web/package.json is the Svelte dashboard, not a docs site. No Starlight docs build exists."
      },
      {
        "file": ".github/workflows/e2e.yml",
        "quote": "name: E2E Tests",
        "detail": "The .github/workflows/ directory contains exactly two files: ci.yml and this e2e.yml. Both were read in full; neither contains any GitHub Pages deployment step (no actions/deploy-pages, no peaceiris/actions-gh-pages, no pages-publish job). There is no documentation deployment pipeline of any kind."
      }
    ]
  }
]
