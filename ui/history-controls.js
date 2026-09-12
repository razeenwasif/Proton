/** @param {boolean} redo @returns {import('./protocol').WireCommand} */
export function historyCommand(redo) {
  return { type: 'perform', action: { verb: redo ? 'Redo' : 'Undo', object: 'None', count: 1, register: null } };
}

/**
 * Called only while the code editor owns shortcuts. Native text fields keep
 * their browser undo history; terminals and overlays are excluded by caller.
 * @param {KeyboardEvent} event
 * @param {import('./protocol').Dispatch} dispatch
 * @param {boolean} mac
 */
export function historyShortcut(event, dispatch, mac) {
  if (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable]:not([contenteditable=false])')) return false;
  if (event.altKey || !(mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)) return false;
  const key = event.key.toLowerCase();
  if (key !== 'z' && !(key === 'y' && !mac && !event.shiftKey)) return false;
  event.preventDefault();
  dispatch(historyCommand(key === 'y' || event.shiftKey));
  return true;
}
