# ccvl

<!-- ccid:security:begin (rendered by ccid render; do not edit by hand) -->
**Security and dependencies:** watched by Renovate (with OSV vulnerability alerts); an update is merged only when its tests pass. See the [security policy](.github/SECURITY.md).
<!-- ccid:security:end -->

[Verification and release checks](.agent/docs/ci.md)

ccvl is a local-first, forkable CV and application system built as a native
Rust binary with an embedded Typst engine. It ships a real bilingual career
profile as its working example, a controllable Harvard-style presentation
layer, strict document checks, and portable agent skills for the complete
application loop.

The showcase is intentionally a real CV rather than fictional sample data. It
demonstrates the system at production quality and makes its author discoverable
for suitable roles. It is reference-only personal content, not reusable
template wording. New users replace it with their own verified facts.

## Showcase and open application

[View the bilingual CV and cover-letter showcase](.agent/docs/showcase.md).
The personal content is visible for professional evaluation, but is not a
reusable template.

## A simple workspace

The product has four domains:

```text
.agent/                                  implementation and agent workflows
inputs/                                  profile, interviews, assets and library
outputs/cvl/                             approved general CV and cover letter
outputs/<document>/                      flyers, web, videos, presentations, portfolios, chatbot, audio
opportunities/<organisation>/<position>/ one concrete job and its documents
```

`.github/` contains contributor and security policy files and `LICENSES/` contains the complete
legal texts. There is no separate market map. General preferences discovered
with the user belong in `inputs/interviews/`; company and role research belongs beside
the concrete job in `opportunities/`.

Included are:

- Harvard German and English CVs with checked two-, three-, and four-page
  variants, a five-line Summary, and a measured target-neutral cover letter;
- [D+ spacing for Harvard](outputs/cvl/cv/harvard/d-plus/README.md) and a
  [cluster CV](outputs/cvl/cv/cluster/README.md) whose first page regroups seven
  approved Harvard stations into finance, technology and consulting sections,
  followed by the unchanged Harvard pages 2–4;
- [a rendered gallery and full folder overview](outputs/cvl/README.md);
- [flyer, web, video, presentation and portfolio scaffolds](outputs/README.md)
  on the same style grid, printing only the profile header until designed;
- one validated `application.toml` per concrete opportunity;
- a shared Rust and Typst engine with bundled fonts and reproducible PDF
  output;
- eight agent skills for setup, evidence-backed profiles, style creation, CV work,
  applications, interview preparation, upskilling, and outcome tracking;
- privacy and provenance rules for keeping personal application data in a
  private downstream repository.

## Quick start

Install the `cv` binary for your platform (Linux, macOS or Windows) from this
repository's latest release, then create your own copy of the complete working
example, the real bilingual CV included. It is written from data inside the
binary, offline:

```sh
cv init my-cv        # writes the full example workspace and records .cv/manifest.json
cd my-cv
cv build             # renders the showcase CVs and letters; no other tool is needed
cv new-opportunity example-org strategy-lead
# Complete the new application.toml with the ccvl-apply skill, then:
cv build-opportunity example-org strategy-lead
```

`cv init` refuses a non-empty directory (`--force` writes into one but never
overwrites an existing file) and offers `git init` without ever committing. It is
easier to change an existing CV than to write a first one: edit the text, keep
what fits, delete what does not.

### Updating

Install a newer `cv`, then, inside your workspace:

```sh
cv update            # shows the plan; changes nothing
cv update --apply    # applies it and prints a summary
```

`cv update` treats files the way dpkg treats configuration files. A file you
never changed is replaced by the new version. A file you changed stays yours and
the new version is written beside it as `<name>.cv-new`; compare, merge what you
want, delete the `.cv-new`. New files are added, files dropped upstream are
removed only if you never touched them, and a file you deleted stays deleted.
Schema migrations run forward and are safe to repeat. A workspace written by a
newer `cv` is refused, with nothing changed. The state lives in
`.cv/manifest.json` (the cv version and a sha256 for every file `cv` wrote); keep
it with your workspace.

### Working from a fork

The code is public and you can still fork it. In a checkout of this repository,
`cv update` adopts it: files identical to the shipped ones are recorded, every
other file is treated as yours, and the tool's own sources are never touched. The
fork's launcher keeps working:

```sh
git clone <ccvl repository URL> ccvl
cd ccvl
bash ./cv setup      # installs the binary matching this source into .agent/cache
bash ./cv check
bash ./cv build
```

What a workspace contains: `inputs/`, `outputs/` (the sources; renders appear
after `cv build`), `opportunities/`, `cv.json`, the agent skills, the docs, the
licence files and the fonts. The tool's own sources, tests and CI are not part
of it: the installed binary is the tool.

The complete novice and terminal workflows are in [Getting
started](.agent/docs/getting-started.md).

Generated general documents are written to
`outputs/cvl/cv/<style>/<substyle>/<language>/<region>/pdf/cv-{2,3,4}.pdf` and
`outputs/cvl/cl/<style>/<substyle>/<language>/<region>/pdf/cl.pdf`.
The shipped Harvard CV substyles are `d-plus` (the default), `standard`, `compact` and `aligned`;
cover-letter substyles are `frame` (the default) and `left-rule`. `bash ./cv list-styles` shows every
style, substyle and empty slot. Use `build-cv en-ch 4 --style harvard --substyle compact` or
`build-cl en-ch --style harvard --substyle frame` with your platform launcher.
Other output documents land in
`outputs/<document>/<style>/<substyle>/<language>/<region>/pdf/<document>.pdf`;
`build-output <document> <locale>` builds any one, for example `build-output flyers en-ch`.
`compare harvard/d-plus cluster/d-plus harvard/compact` renders any variants into one
side by side review PDF; see [.agent/docs/compare.md](.agent/docs/compare.md).
Other styles own their layouts and page presets; see [.agent/docs/styles.md](.agent/docs/styles.md).
Opportunity-specific documents are written beside their job record under
`opportunities/<organisation>/<position>/pdfs/` (rendered PDFs) and
`opportunities/<organisation>/<position>/typst/` (resolved standalone
Typst copies).

The Linux x86_64 bundle includes the tested, optimized executable and its
matching workspace. Its ordinary Linux runtime is verified on Ubuntu 24.04
with glibc 2.39. Linux ARM64, macOS and Windows have no verified prebuilt release.
Git, Rust, and an external Typst installation are unnecessary for normal use.

For an existing checkout, setup downloads the checksum-verified runtime that
matches its compiler source. It rejects older binaries, and every launcher
checks the installed runtime again before executing a command. Personal CV,
interview, and opportunity changes do not require recompilation. Developers
can explicitly opt into a locked source build with `bash ./cv setup
--from-source` on Linux/macOS or `.\cv.cmd setup --from-source` on Windows.
These native developer paths do not establish binary release support on those
other platforms.

The [release contract](.agent/docs/releases.md) describes the required native
builds, shared artifacts, dependency caches, and publication gate.

## Make it yours

The workflow moves in one direction:

```text
inputs/interviews/ -> outputs/cvl/ -> opportunities/<organisation>/<position>/
     ^                         |
     +----- verified facts ----+
```

Start with `ccvl-profile`: import sources or answer one question at a time
while the agent maintains `inputs/interviews/profile.md`, `inputs/interviews/journal.md`, and
`inputs/interviews/stations.toml`. Once the evidence and fixed station layout are
complete, replace the general showcase under `outputs/cvl/` with approved wording.
Create one directory for each concrete job; its posting, research, tailored
documents, interview preparation, submission record, and outcome all stay
together.

A private standalone repository may retain ccvl as `upstream`; a public fork
must remove the original author's personal content before publishing its
replacement. See [Private downstreams](.agent/docs/private-downstream.md).

The checked-in showcase is visual and implementation evidence, never evidence
about another user. Claims may be selected and compressed from that user's own
evidence, but must never be invented.

## Agent workflows

Canonical skills live under `.agent/skills/`:

- `ccvl-install`
- `ccvl-profile`
- `ccvl-style`
- `ccvl-cv`
- `ccvl-apply`
- `ccvl-interview`
- `ccvl-upskill`
- `ccvl-outcome`

Repository-wide operating rules are in [AGENTS.md](AGENTS.md). The [data
model](.agent/docs/data-model.md), [testing contract](.agent/docs/testing.md),
and [skill map](.agent/docs/skills.md) document the boundaries in detail.

## License

ccvl uses path-specific licensing:

- software, Typst sources, scripts, and skills: FSL-1.1-ALv2;
- documentation and neutral scaffolds: CC-BY-4.0;
- personal showcase data and Typst content, generated signature, and rendered
  showcase PDFs: LicenseRef-CCVL-Personal-Content;
- bundled fonts: OFL-1.1.

FSL is a Fair Source license, not an OSI Open Source license. Each published
version becomes available under Apache-2.0 two years after publication. See
[LICENSE.md](LICENSE.md) and [REUSE.toml](REUSE.toml) for the exact mapping.
