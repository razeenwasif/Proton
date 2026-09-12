/** @typedef {import('./protocol').WireEvent} Event */
/** @typedef {Extract<Event, {type: 'viewport'}>} Viewport */
/** @typedef {Extract<Event, {type: 'cursor'}>} Cursor */
/** @typedef {{firstLine: number, lines: string[], totalLines: number,
 * cursor: {line: number, column: number}, selection: Cursor['selection'],
 * highlights: Viewport['highlights'], matches: Viewport['matches'], git: Viewport['git'],
 * stops: Map<number, Viewport['stops'][number]>, painted: string[] | null}} Pane */

export function createEditorState() {
  /** @type {Map<number, Pane>} */
  const panes = new Map();
  /** @param {number} index @returns {Pane} */
  function pane(index) {
    let value = panes.get(index);
    if (!value) {
      value = {firstLine: 0, lines: [], totalLines: 0, cursor: {line: 0, column: 0},
        selection: null, highlights: [], matches: [], git: [], stops: new Map(), painted: null};
      panes.set(index, value);
    }
    return value;
  }
  /** @param {Viewport | Cursor} event */
  function update(event) {
    const value = pane(event.view);
    if (event.type === 'cursor') {
      value.cursor = {line: event.line, column: event.column};
      value.selection = event.selection;
    } else {
      value.firstLine = event.firstLine;
      value.lines = event.lines;
      value.totalLines = event.totalLines;
      value.highlights = event.highlights;
      value.matches = event.matches;
      value.git = event.git;
      value.stops = new Map(event.stops.map(stop => [stop.line, stop]));
    }
  }
  return {panes, pane, update};
}

/**
 * Apply a whole core transaction before drawing. No animation-frame delay:
 * viewport, cursor, diagnostics and layout can request the same pane, but it
 * paints once with the final state before dispatch resolves.
 * @param {(view: number) => void} paint
 */
export function createEditorPaints(paint) {
  let batching = false;
  /** @type {Set<number>} */
  const dirty = new Set();
  return {
    begin() { batching = true; },
    /** @param {number} view */
    request(view) { if (batching) dirty.add(view); else paint(view); },
    flush() {
      batching = false;
      const views = [...dirty];
      dirty.clear();
      views.forEach(paint);
    },
  };
}
