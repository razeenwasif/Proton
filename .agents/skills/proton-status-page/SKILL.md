---
name: proton-status-page
description: >-
  Maintains Proton's interactive HTML Status & Plan dashboard in sync between
  the local repo copy (docs/status.html) and an editable Antigravity HTML
  artifact (proton_status_plan.html). Use whenever updating Proton's project
  status, milestones, modes table, backlog, guide, cheatsheet, or installed app
  state.
---

# Proton Status & Plan Dashboard Maintenance

The legacy Claude.ai artifact (`https://claude.ai/artifact/MJbbNCvkxGChBXSy1U9ToM`)
is read-only from Antigravity and superseded. Maintain Proton's interactive HTML
Status & Plan dashboard in two synchronized locations instead:

1. **Canonical Repo Copy:** `docs/status.html` (version-controlled via `ntr`).
2. **Antigravity Artifact Copy:** `<artifactDir>/proton_status_plan.html` in the
   active conversation's artifact directory (self-contained interactive HTML,
   zero external CDN dependencies).

## Architecture

- Self-contained single-file HTML (inline CSS, inline SVG diagrams, inline JS).
- Plan step statuses live in `INITIAL_STATUSES` inside the `<script>` block
  (replacing Claude's `window.claude.use("db")` cloud store), with interactive
  browser overrides persisted in `localStorage` under `proton-status-plan:v1`.
- Includes four interactive sections: **Plan** (progress bar, phase counters,
  editable step status dropdowns), **Backlog** (filterable by `Waiting on you` /
  `Later builds` + search), **Guide** (SVG window & project-switching diagrams,
  mode cards), and **Cheatsheet** (searchable keyboard reference).

## Update Workflow

1. **Gather live state:**
   - Run `export PATH="$HOME/.local/bin:$PATH" && ntr log -n 5 --oneline`,
     `ntr rev-list --count master`, and `ntr worktree list` to verify `master`
     HEAD, `origin/master` (Neutrino on `atlas`), and active worktrees/branches.
   - Check `/Applications/Proton.app/Contents/MacOS/proton-shell` timestamp to
     confirm which commit is installed.
   - Read `docs/BACKLOG.md` if backlog items changed.
2. **Update `docs/status.html`:**
   - Update hero facts (`master` SHA, commit count, date, active worktrees) and
     the four **Vitals** cards (`Neutrino · GitHub`, `CI`, `Installed app`,
     `Tests`).
   - Update the **Modes** table, `STEPS` array, and `INITIAL_STATUSES` map.
   - Update Backlog items/counts, Guide cards, or Cheatsheet rows as needed.
3. **Validate JavaScript:**
   - Extract all `<script>` blocks and run `node --check` (with
     `/opt/homebrew/bin` on `PATH`) to ensure zero syntax errors.
4. **Sync the Antigravity artifact:**
   - Copy the updated `docs/status.html` to `<artifactDir>/proton_status_plan.html`
     and link it in chat so the user can open it directly.
5. **Commit policy:**
   - Only commit/push `docs/status.html` via `ntr` when the user asks; push only
     to `origin` (Neutrino on `atlas`), never directly to GitHub.
