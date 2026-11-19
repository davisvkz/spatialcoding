# SpatialCoding

SpatialCoding lays a codebase out on an infinite canvas. Every file is a node with its real, colored code, placed inside its folder, and arranged left to right by who imports whom. Lines (edges) connect an import to the export that resolves it.

## Open code

- Run `spatialcoding <folder>` or press Ctrl+Shift+O after opening a folder.
- Drag nothing: use the Workspace panel (Ctrl+Shift+E): right click a file or folder, Open on canvas.
- Open all text files with Ctrl+Shift+O; clear the canvas with Shift+X.

## Move around

- Pan: Space + drag, or right-drag; trackpad: two fingers.
- Zoom: Ctrl + scroll, pinch or Ctrl +/-. Zoom goes from 0.1 % to 100 %.
- F zooms to 60 % at the mouse; Shift+F fits everything.
- Shift+V shows the zoom level (L1 text, L2 token rectangles, L3-L5 CST tint only).
- M toggles the minimap.

## Folders

- R collapses the leaf folders, E expands the topmost collapsed ones.
- Shift+E / Shift+R expand / collapse the folder under the mouse (all folders without one).
- Ctrl+E / Ctrl+R expand / collapse the whole level of the folder under the mouse.
- `\` selects the open files of the folder, Shift+`\` opens the rest of them.

## Edges

- Selecting a file or symbol turns its edges light blue; red edges run backwards (layout violation or cycle).
- The Dependency Edges panel sets style (Bezier, Step, Line, PCB), opacity, arrow heads and the gradient gap.

## Panels

- Ctrl+K opens the command palette; hover a command to see where its control lives (Go to UI).
- Ctrl+. hides every panel, Ctrl+S hides and restores the open ones.
- Ctrl+/ lists every shortcut.
