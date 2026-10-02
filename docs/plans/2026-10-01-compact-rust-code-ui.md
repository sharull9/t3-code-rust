# Compact Rust code desktop UI

The [October 2 migration plan](2026-10-02-full-t3-code-migration.md) supersedes
this document's sidebar settings placement and 40% height limit. Settings now
occupies the main area with category navigation. The other compact UI decisions
below remain the record of the earlier change.

The supplied screenshots identify excess timeline spacing, oversized Copy controls,
a collapsed project list, and an empty folder-browser request. This pass keeps the
native desktop structure and makes its controls compact and predictable.

## Design and behavior

- Use a dark utilitarian surface with a restrained copper accent. Windows uses
  Bahnschrift for readable, compact interface text. The header and window title
  read `Rust code`.
- Keep the sidebar toggle left, breadcrumbs in the center, and workspace/settings
  icons right. Header controls block drag hit testing so clicks operate the controls.
- Cap Settings at 40% of viewport height, anchored to the sidebar bottom. Keep Done
  visible and scroll the contents. Active threads scroll independently of the footer.
- Override the transcript scroller's default 32px row padding with 6px and tighten
  inner padding. Copy is an icon-only control aligned to the content's right edge.
- Group activity runs between messages. Expand the run to see each tool/status item,
  then expand an individual item for its payload. Preserve reasoning/message order.
- Keep the composer as one bordered surface: selected attachment tray, input, then
  paperclip/model/mode/send controls. Hide the tray when empty and wrap narrow toolbars.
- Give the project chooser a definite list height. Selecting a visible project
  dispatches thread creation for that project. Folder browsing begins at the selected
  project root or the server home directory (`~`); blank manual input shows a local validation message.

## Verification

83 tests pass (82 unit/interaction tests and one documentation test).

Native interaction tests cover copy width and row spacing, tool run/item expansion,
project selection and creation dispatch, folder request validation, header toggle
clicks, settings placement/height, focus restoration, attachment selection controls,
and active-thread scrolling with a stable footer. Live readonly backend checks use
`~` without a current project to verify the folder browser's initial path against the actual server.

Orca could not provide desktop screenshots: `orca open --json` returned
`runtime_open_timeout` while waiting for its desktop window. Layout assertions use
native headless bounds and clicks; final pixel inspection remains manual.
