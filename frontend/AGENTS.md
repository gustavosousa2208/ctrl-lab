# Frontend Subproject Notes

- Simulation behavior must remain decoupled from visuals.
- Block presentation should stay compact and grid-aligned: use a single visible name, avoid duplicated labels, and avoid descriptive body copy inside canvas nodes.
- Sort any menu alphabetically, and use a consistent order for menu items across the app.
- Top bar content should always be aligned on the horizontal
- Top bar editor-related content should be aligned to the left, simulation-related content should be aligned to the left, but on the right side of the editor-related content.
- Preserve startup-safe React Flow usage. Do not reintroduce mount-time feedback loops by coupling derived node arrays, viewport callbacks, and state writes without guards.
- Treat React Flow `nodes` and `edges` as the source of truth. If live signal data or UI-only data is needed, derive it outside the stored graph state instead of rebuilding node objects every render.
- Any selection work must keep these behaviors working together:
  - multi-select deletion with `Delete`
  - single-click inspector open
  - `Ctrl+Z` / `Cmd+Z` undo for graph edits only
- Do not push project lifecycle actions into canvas undo history. `New`, `Open`, and `Close Project` must not make undo jump between home/project states.
- Duplicate drag is fragile. `Ctrl` / `Cmd` drag must leave the original fixed in place and move the duplicate, not the reverse, and any React Flow release-position updates must be guarded against.
- Viewport persistence is part of the project contract. Save and restore:
  - zoom
  - viewport x
  - viewport y
  - zoom step
  Older project files must still open safely when these fields are missing.
- Cursor behavior matters:
  - empty canvas uses drag/grab
  - blocks and block I/O use precise selection
- Non-editable editor chrome must stay non-selectable. Text selection should remain enabled only inside actual form controls.
- Top bar numeric fields and bottom bar readouts must keep consistent font treatment and vertical alignment. Avoid browser-default number input styling drifting from the custom UI chrome.
- Before finishing frontend changes, verify at minimum:
  - app opens without React Flow runtime loops
  - drag from rack inserts one block only
  - `Ctrl` / `Cmd` drag duplicates correctly
  - `Delete` removes the actual current selection
  - `Ctrl+Z` / `Cmd+Z` undoes graph edits without closing the project
  - save/open preserves viewport
  - top bar stays in two left-aligned groups, editor chrome first and simulation
    controls after it, with the divider visible between them
  - top bar indicators and bottom bar readouts share font family and size with
    their labels
