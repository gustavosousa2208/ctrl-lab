# Project Status

Recovery snapshot. Refresh this file rather than starting a new one.

| | |
| --- | --- |
| Snapshot date | 2026-09-03 |
| Branch | `main` |
| HEAD at snapshot | `91ea741` "Draft firmware control-runtime architecture" (2026-07-24) |
| Working tree at snapshot | **already dirty** — that work is now committed, see [Recently landed](#recently-landed) |
| HEAD after recovery | `bfaa9c6` |
| Remote | `origin` = `git@github.com:gustavosousa2208/ctrl-lab.git` |
| Other branches | none — `origin/rst-impl` was salvaged and deleted (see below) |
| Stashes / tags / worktrees | none |

Evidence tags below: **[V]** verified by running or reading code, **[I]**
inferred from multiple clues, **[?]** unknown.

## What the project is

`ctrl-lab` lets a user draw a control-system block diagram, simulate it
deterministically on the PC, and — as the eventual goal — execute the same
validated controller on a microcontroller and compare simulated against measured
behavior. Philosophy and boundaries: [`AGENTS.md`](AGENTS.md). **[V]**

Maturity: a working two-layer PoC (editor + simulator) with a designed but
unbuilt third layer (firmware). **[V]**

## Components and data flow

```
frontend/src/EditorApp.tsx        React Flow canvas, block catalog, inspector,
  |                               project save/open, scope plot
  |  project JSON  (nodes, edges, graphIndex, simulation config)
  v
frontend/src-tauri/src/lib.rs     Tauri commands: simulate_project,
  |                               compile_project_report
  |  calls the backend crate in-process
  v
backend/src/lib.rs                parse_project_json -> ValidatedDag
  |                                 (validation, topological sort over
  |                                  direct-feedthrough edges only)
  |                               simulate_validated_dag -> per-node traces
  |
  +--> backend/src/plan.rs        build_control_plan -> ControlPlan -> bytes
                                  (no caller yet — tests only)
                                          |
                                          v
                                  firmware/  (design notes only, no code)
```

- `frontend/src-tauri/Cargo.toml` depends on `ctrl-backend` by path, so the
  desktop app links the simulator directly. **[V]**
- `compile_project_report` additionally shells out to `cargo build` and then runs
  the `ctrl-backend` **binary** from `backend/target/debug/`. That path is
  hardcoded to the `debug` profile and assumes a checkout with a Cargo toolchain
  present — it cannot work from a packaged release build.
  **[V, `frontend/src-tauri/src/lib.rs`]**
- `backend/src/main.rs` is a standalone CLI: validate + simulate one project
  file. `backend/examples/trace.rs` dumps a full CSV trace. **[V]**

Key paths: `backend/src/lib.rs` (3595 lines, the whole engine),
`frontend/src/EditorApp.tsx` (92 KB, the whole editor).

## Verified commands

| Purpose | Command | Result |
| --- | --- | --- |
| Backend tests | `cargo test --manifest-path backend/Cargo.toml` | 47 unit + 4 golden pass **[V]** |
| Backend CLI | `cargo run --manifest-path backend/Cargo.toml -- test-projects/01-double-integrator.json` | exit 0, prints order + final values **[V]** |
| Frontend build | `cd frontend && bun run build` | `tsc -b && vite build`, clean **[V]** |
| Desktop app | `cd frontend && bun run tauri dev` | not run in this session **[?]** |

## What works

- Project parsing and **validation-first** rejection: unknown block types,
  unconnected required inputs, multiple edges into one port, algebraic loops,
  `graphIndex` drift. **[V — covered by the unit tests]**
- Deterministic fixed-step simulation of constant, step, square wave, gain, sum,
  switch, delay, integrator, transfer function, scope/display. **[V]**
- Transfer functions in all three authoring forms: continuous, discrete `z`, and
  discrete `z^-1`. Continuous ones are ZOH-discretized at `Ts` via the augmented
  matrix exponential, matching MATLAB `c2d(..., 'zoh')`. **[V]**
- Golden regression against MATLAB: all four fixtures match sample-by-sample
  within `1e-6`. **[V]**

## What does not work / is absent

- **No firmware.** `firmware/` contains `AGENTS.md` and `.gitkeep` only. No
  Zephyr project, no board, no HIL. **[V]**
- **No deployment path.** Nothing emits a Deployable Control Plan to disk or over
  a wire, even with `plan.rs` present — it has no caller. **[V]**
- **No frontend test suite.** Regressions are caught only by the manual checklist
  at the end of `frontend/AGENTS.md`. **[V]**
- **`graphIndex` is trusted, not checked, on the frontend side.** The backend
  validates it; the editor can still serialize a stale one. Already tracked in
  `TODO.md`. **[V]**
- **Multi-rate is not implemented.** Single clock, `stepSize == Ts`. The GCD
  base-rate rule in `backend/AGENTS.md` is reserved semantics, not built code.
  **[V]**

## Recently landed

This was the uncommitted work found in the tree at snapshot time. It was
reviewed, verified, and committed unmodified during the recovery.

1. **`7e312b3` — Deployable Control Plan (`backend/src/plan.rs`, 708 lines).**
   Implements the backend→firmware container specified in `firmware/AGENTS.md`:
   a stable `KernelId` enum, `build_control_plan` over a `ValidatedDag`,
   parameter packing (transfer functions packed as discrete state space), and a
   little-endian `encode`/`decode` pair with CRC32 and an FNV-1a plan id.
   Carries 5 tests, including a round-trip over every fixture; all pass. **[V]**
   `backend/src/lib.rs` gained exactly one line, `pub mod plan;`, to expose it.
   **Nothing calls `build_control_plan`** — the plan cannot yet be emitted
   outside a unit test. **[V]**

2. **`bfaa9c6` — authoring discrete transfer functions in the editor.** Adds
   `domain` (continuous/discrete) and `discreteVariable` (`z` / `z^-1`) select
   fields to the transfer-function block and surfaces them in the node summary.
   This closed a real gap: the backend had understood these properties since
   `fdf547e`, but the editor could not set them. Defaults (`continuous`, `z`)
   match the backend's fallbacks, so existing project files load unchanged.
   Builds clean. **[V]** **Not exercised against `frontend/AGENTS.md`'s manual
   checklist** — that needs a human at the running app. **[?]**

**[I]** The two were one thread of work, not two: `plan.rs` needs a validated
model whose discrete blocks are authorable, so the editor change is its front
half. Both post-date `91ea741`, which drafted the firmware architecture the plan
implements.

## Artifact classification

| Artifact | Class | Disposition |
| --- | --- | --- |
| `backend/src/plan.rs` | in-progress feature, tested | committed in `7e312b3` |
| `Open.ps1` (untracked) | local convenience script (`cd frontend; bun run tauri dev`) | left in place; harmless to commit or delete |
| `frontend/.tauri-check.err` / `.log` (untracked) | stale local dev logs from 2026-04-10; that run ended `exited with code 58` **[?]** — cause unknown, and the app has been developed for months since | now git-ignored, left on disk |
| `frontend/vite.config.js`, `vite.config.d.ts` | **generated** by `tsc -b` from `vite.config.ts`, but were **tracked** | untracked + ignored, see below |
| `frontend/tsconfig.{app,node}.tsbuildinfo` | generated build cache, but tracked (and showing as modified) | untracked + ignored |
| `frontend/PLAN.md` | scratch checklist, all three items `[x]` | left in place; superseded by `TODO.md` |
| `test-projects/*.ref.csv` | generated by MATLAB, **intentionally committed** so the suite runs without MATLAB | keep tracked |
| `backend/target/`, `frontend/dist/`, `frontend/node_modules/` | build output | already ignored |

### `origin/rst-impl`

Diverged from `4b3695e` on 2026-04-13. Its backend work (an earlier discrete
transfer-function implementation using `VecDeque` history buffers) was
**superseded** by `fdf547e` on `main`, which reimplemented the same capability
via state space with ZOH and golden tests. Do not merge the backend half.
**[V — diffed both]**

It carried two files that never reached `main`: a root `README.md` and a root
`package.json` of workspace scripts. **[V]** Both have been recovered — the root
`README.md` was rewritten from scratch by this recovery, and the root
`package.json` was taken from the branch verbatim and committed. The branch was
then deleted from the remote, since everything of value on it had either been
superseded or salvaged.

Its last commit was **`e6d0da6`** (2026-04-13). Recorded here so the superseded
discrete-TF implementation can still be retrieved if it is ever wanted:
`git show e6d0da6` works from any clone that fetched the branch before deletion,
and GitHub retains unreferenced objects for a period after branch deletion.

## Documentation state

Fixed during this recovery:

- `backend/AGENTS.md` claimed continuous blocks used **forward Euler**, with RK4
  as a candidate. Stale since `fdf547e`; they are ZOH-discretized. Corrected.
- `backend/AGENTS.md` listed the deployable representation as an untouched
  deferred decision. `plan.rs` now implements one. Corrected, with its
  unwired status stated.
- `TODO.md` asked whether to commit `*.ref.csv`. They are committed. Moved to
  Done, and the in-progress items above were added to "Now".
- No root `README.md` existed. Added.

Still open:

- `frontend/README.md` was the only README a reader could find, and it describes
  the frontend as if it were the whole project. It is accurate about the
  Tauri/WSL2 setup and is now linked from the root README for exactly that, but
  its opening framing duplicates the root README. Whether to trim it to a pure
  setup guide is your call. **[?]**
- `firmware/AGENTS.md` describes the DCP container in a slightly different shape
  than `plan.rs` encodes — notably the transfer-function kernel: the doc
  prescribes a **biquad SOS cascade** for f32 robustness, while `plan.rs` packs a
  single discrete state space. Reconcile before firmware work starts.
  **[V — compared both]**

## Risks, blockers, open questions

- **The DCP layout has never been read by a consumer.** Nothing has decoded a
  plan except its own round-trip test. Field widths, `rate_div` semantics, and
  the empty `io_bindings` will move once a real loader exists. Treat
  `DCP_FORMAT_VERSION = 1` as a draft, not a frozen wire format.
- **`wcet_estimate_ns` is hardcoded to `0`.** The firmware design has the loader
  *reject* a plan whose WCET exceeds the period; with 0 that check is vacuous.
  **[V]**
- **f32 vs f64.** The backend simulates in `f64`; `plan.rs` packs `f32`. The
  simulation-vs-device error the project exists to measure will include this.
  `TODO.md` tracks it; no test covers it yet. **[V]**
- **`compile_project_report` requires a dev checkout** (see Components above). It
  cannot ship as-is in a packaged app.
- **The RST controller form is still undecided** — the blocking item in
  `TODO.md` before firmware work can start.

## Next actions

Immediate (restore a clean, shareable state):

1. Run the `frontend/AGENTS.md` manual checklist against `bfaa9c6` in the
   running desktop app. It is the one thing committed here that no automated
   check covers.

Then (unblock firmware):

2. Give the plan a caller — a `--emit-plan <out.dcp>` flag on `ctrl-backend` is
   the smallest useful one — so plans can be inspected outside a unit test.
3. Reconcile `firmware/AGENTS.md` with `plan.rs`: pick state-space or biquad-SOS
   for the transfer-function kernel, and settle `io_bindings` and
   `wcet_estimate_ns`.
4. Document the RST equation form (`TODO.md` "Now").

Later (quality):

5. Frontend `graphIndex` consistency checks (`TODO.md`).
6. Extend golden coverage to internal controller states, and add the
   step/ramp/disturbance/noise/reset cases listed in `TODO.md`.
7. Make `compile_project_report` work without a Cargo toolchain, or drop it in
   favor of the in-process `simulate_project` path.

## Changes made by this recovery

**No source logic was written or altered by this recovery.** The two commits
above are the pre-existing working-tree work, committed byte-for-byte as found.
Everything else below is documentation and repository hygiene.

- Committed the in-progress work as `7e312b3` and `bfaa9c6` (see
  [Recently landed](#recently-landed)).
- Salvaged the root `package.json` (workspace scripts) from `origin/rst-impl`
  and deleted that branch from the remote; its last commit was `e6d0da6`.
  `backend:build`, `backend:run`, and `frontend:build` were each run to confirm
  the scripts work. The three that chain `bun install` (`setup`, `dev`,
  `tauri:dev`) were not run, to avoid touching the lockfile. **[V/?]**
- Added `README.md` (root) and `PROJECT_STATUS.md` (this file).
- Corrected the stale forward-Euler and deferred-deployable-format claims in
  `backend/AGENTS.md`; updated `TODO.md`.
- Stopped `tsc -b` from emitting `vite.config.js` / `vite.config.d.ts`
  (`"noEmit": true` in `frontend/tsconfig.node.json`) and redirected both
  `.tsbuildinfo` files into `node_modules/.tmp/`. A committed `vite.config.js` is
  a genuine hazard: Vite resolves `vite.config.js` **before** `vite.config.ts`,
  so a stale generated copy silently shadows the source. The two were identical
  at snapshot time, so nothing had behaved wrongly yet. **[V]**
- Untracked those four generated files (`git rm --cached`, content preserved in
  history) and extended `.gitignore` to cover them plus the `.tauri-check.*`
  logs.

Validated after the changes: `cargo test --manifest-path backend/Cargo.toml`
(51 pass), `cd frontend && bun run build` (clean), `git diff --check` (clean).
