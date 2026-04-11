# AGENTS.md

## Purpose

`ctrl-app` is a proof of concept for reducing the gap between control-system simulation and real-time embedded execution.

The goal is to let users familiar with Simulink-style workflows model a controller, simulate it on the PC, deploy it to a microcontroller running Zephyr-based firmware, and compare simulated results against measured real-time behavior.

This is not a full Simulink replacement. It is a focused platform for validating model-to-embedded execution with measurable metrics.

## Subprojects

### Frontend

Owns:

- UI
- canvas
- block library
- editor
- project/file management
- visualization

Must remain responsive and reliable.  
Must not own simulation or real-time logic.

### Backend

Owns:

- PC-side simulation
- model validation
- transformation to deployable representation
- communication with the microcontroller
- telemetry routing
- metric calculation and comparison

This is the orchestration layer between frontend and firmware.

Implementation constraints:

- Backend foundation and performance-critical backend logic must be written in Rust.
- Prefer explicit, versioned data contracts between frontend and backend.
- Keep parsing, validation, optimization, simulation, and deployable-model generation in backend-owned Rust modules.

### Firmware

Owns:

- deterministic execution on the microcontroller
- signal processing and actuation
- telemetry generation
- execution of validated deployable representations

The firmware should act as a stable runtime, not require reflashing for routine model changes.

## Architectural rules

- Frontend, backend, and firmware must be decoupled and communicate through explicit contracts.
- The frontend must not depend on firmware internals.
- The backend must not contain UI concerns.
- The firmware must not depend on frontend concepts.
- Real-time requirements increase toward the microcontroller:
  - frontend: no real-time guarantees
  - backend: soft real-time at most
  - firmware: deterministic execution required
- Logic should live in one layer only. Avoid duplicated responsibility across layers.

## Development principles

- Prefer familiar Simulink-style interaction, not feature parity.
- Prioritize PoC depth over feature breadth.
- Prefer a small number of reliable blocks over broad unsupported coverage.
- Do not add features that weaken determinism or blur architectural boundaries.
- The backend must reject invalid or ambiguous models before deployment.
- The firmware must execute only constrained, validated, versioned representations.
- Performance claims must be measurable.

## Key metrics

Whenever possible, expose and compare:

- control step time
- worst-case execution time
- jitter
- communication latency
- telemetry throughput
- simulation vs embedded output error

## Non-goals

This project is not trying to:

- replicate full Simulink coverage
- support arbitrary code execution on the device
- hide all embedded-system constraints
- guarantee real-time behavior on the desktop side

## Decision filter

Before adding a feature, ask:

1. Does this reduce the gap between simulation and embedded execution?
2. Does this preserve or improve determinism where it matters?
3. Does this respect frontend/backend/firmware boundaries?
4. Can it be measured or validated?
5. Is it necessary for the PoC?

If most answers are no, do not add it.
