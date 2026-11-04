# GitHub community setup (maintainer checklist)

The files in the repository (issue forms, PR template, Code of Conduct,
security policy) work as soon as they are on `main`. The items below are
**repository settings on github.com**, which only the maintainer can change.
None of them has been changed.

State checked on 2026-11-04 with the GitHub CLI:

| Setting | State | Command used |
|---|---|---|
| Visibility | Public | `gh repo view --json visibility` |
| Issues | Enabled | `gh repo view --json hasIssuesEnabled` |
| Discussions | **Disabled** | `gh repo view --json hasDiscussionsEnabled` |
| Private vulnerability reporting | **Disabled** | `gh api repos/davidfdzmorilla/pulse/private-vulnerability-reporting` |
| Topics | **None** | `gh repo view --json repositoryTopics` |
| Website | Empty | `gh repo view --json homepageUrl` |
| Community profile | 57 % (before this branch: README, CONTRIBUTING, LICENSE) | `gh api repos/davidfdzmorilla/pulse/community/profile` |
| Labels | GitHub's defaults plus `accessibility`; the 8 labels of section 3 created on 2026-11-04 | `gh label list` |

## Owner actions at a glance

Only the repository owner can do these. Until they are done, the docs say
so openly (a visible note in `SECURITY.md`):

| # | Action | Where | Blocks |
|---|---|---|---|
| 1 | ~~Code of Conduct contact~~ **Done:** `correo@davidfdzmorilla.dev` (also the e-mail fallback in `SECURITY.md`) | — | — |
| 2 | Enable **Private vulnerability reporting** | Settings → Code security | The reporting path in `SECURITY.md` |
| 3 | Legal review of the extra license clause (point 3) in `CONTRIBUTING.md` | a lawyer; questions in [PROJECT_MODEL.md](PROJECT_MODEL.md#open-questions-for-the-maintainer-not-legal-advice) | Merging the first outside contribution |
| 4 | Enable Discussions and add topics (labels: **done** on 2026-11-04) | sections 2 and 4 below | Community |

## 1. Before announcing anything

- [x] **Code of Conduct contact:** `correo@davidfdzmorilla.dev`.
- [ ] **Private vulnerability reporting.** Settings → *Code security* →
  *Private vulnerability reporting* → **Enable**. `SECURITY.md` points to
  it. Then remove the `MAINTAINER ACTION REQUIRED` comment in `SECURITY.md`.

## 2. Discussions

Settings → *General* → *Features* → tick **Discussions**. Then, in the
Discussions tab → ✏️ next to *Categories*, create:

| Category | Format | Purpose |
|---|---|---|
| 📣 Announcements | Announcement (only maintainers post) | Releases, plans |
| 💬 General | Open-ended discussion | Anything about Pulse |
| 💡 Ideas & Feature Requests | Open-ended discussion | Ideas before they become issues |
| 🎹 Music Production | Open-ended discussion | Techniques, workflows, tracks made with Pulse |
| 🛠️ Development | Open-ended discussion | Architecture, code, contributing |
| 🔌 Plugin Compatibility | Open-ended discussion | Which VST3/CLAP plugins work |
| 🎧 Show & Tell | Open-ended discussion | Share what you made |
| 🙋 Support | Question / Answer | How do I…? (answers can be marked) |

Afterwards:
- Add the Discussions link in `README.md` and `README.es.md` (an HTML comment
  marks the spot in the quick links).
- In `.github/ISSUE_TEMPLATE/config.yml`, add a contact link
  `https://github.com/davidfdzmorilla/pulse/discussions/categories/support`
  for questions. You can also set `blank_issues_enabled: false` to send
  everything through the forms.

## 3. Labels

**Done on 2026-11-04**: these labels exist. The commands stay here for
reference (or for a fork). The issue forms and `docs/FIRST_CONTRIBUTIONS.md`
use them.
Existing labels (`bug`, `enhancement`, `documentation`, `good first issue`,
`help wanted`, `accessibility`) are kept.

```bash
gh label create audio-engine      --color 5319e7 --description "Real-time engine, instruments, effects, mixer"
gh label create plugins           --color 1d76db --description "VST3 / CLAP hosting and plugin compatibility"
gh label create midi              --color 0e8a16 --description "MIDI input, controllers, MIDI editing and files"
gh label create ui                --color c5def5 --description "Interface and interaction"
gh label create macos             --color 000000 --description "Specific to macOS"
gh label create windows           --color 0078d4 --description "Specific to Windows"
gh label create performance       --color fbca04 --description "CPU, memory, latency, dropouts"
gh label create "needs reproduction" --color d4c5f9 --description "Can't be reproduced yet: steps or a project file needed"
```

## 4. Repository details

- [ ] **Topics** (Settings → General, or the ⚙️ next to *About*): `daw`,
  `music-production`, `audio`, `rust`, `egui`, `vst3`, `clap`, `midi`,
  `audio-to-midi`, `music`.
- [ ] **Social preview image** (Settings → General → *Social preview*):
  1280×640. `docs/capturas/arrangement.png` with the logo works well.
- [ ] **Description.** The current one is long. A shorter alternative that
  matches the README: *"An independent DAW for creative music production,
  written in Rust. Own audio engine, VST3/CLAP, local audio-to-MIDI, stems
  and mix tools."*

## 5. Funding

There is no `.github/FUNDING.yml` because no sponsorship account is defined.
If you open GitHub Sponsors, Ko-fi, Open Collective or similar, create:

```yaml
# .github/FUNDING.yml — only the platforms you actually use
github: [your-github-username]
```

## Publishing the first issues

`docs/FIRST_CONTRIBUTIONS.md` has five ready-to-publish issues. After
creating the labels (section 3), publish each one with its title, its body
copied from the document, and the labels listed under its heading. For
example:

```bash
gh issue create --title "English versions of the install and platform docs" \
  --label documentation --label "good first issue" --label "help wanted" \
  --body-file <file with the section's text>
```

Then link each issue number back in `docs/FIRST_CONTRIBUTIONS.md`.

## Branch protection

The `.githooks/pre-push` hook only protects your own clone. With outside
contributors, consider a ruleset on `main` (Settings → Rules → Rulesets):
require a pull request and require the `Windows` and `macOS` checks to pass.
This is available on public repositories on the free plan.
