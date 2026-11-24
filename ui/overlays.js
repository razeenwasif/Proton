// View-only overlay lifecycle. Core-owned picker/command cancellation stays
// with the caller; hiding a view must not invent a core command.
export function createOverlays(root, ids) {
  let active = null;
  let previousFocus = null;
  const app = root.querySelector('.app');
  const scrim = root.querySelector('#scrim');
  const modal = (id) => id === 'settings' || id === 'telescope' || id === 'colab-modal';
  const focusable = (panel) => [...panel.querySelectorAll(
    'button, input, select, textarea, a[href], [tabindex="0"], [contenteditable="true"]',
  )].filter((el) => !el.disabled && el.getClientRects().length && !el.closest('[hidden]'));

  function hide(restore = true) {
    ids.forEach((id) => root.getElementById(id)?.classList.remove('on'));
    scrim.classList.remove('on');
    app.inert = false;
    active = null;
    if (restore && previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
    if (restore) previousFocus = null;
  }

  function show(id) {
    if (active === id) return;
    if (!active) previousFocus = root.activeElement;
    hide(false);
    active = id;
    const panel = root.getElementById(id);
    panel.classList.add('on');
    scrim.classList.add('on');
    app.inert = modal(id);
    if (modal(id)) (focusable(panel)[0] ?? panel).focus({ preventScroll: true });
  }

  function tab(event) {
    if (event.key !== 'Tab' || !modal(active)) return false;
    event.preventDefault();
    const panel = root.getElementById(active);
    const items = focusable(panel);
    const at = items.indexOf(root.activeElement);
    const next = at < 0 ? (event.shiftKey ? items.length - 1 : 0)
      : (at + (event.shiftKey ? -1 : 1) + items.length) % items.length;
    (items[next] ?? panel).focus({ preventScroll: true });
    return true;
  }

  return { show, hide, tab, get active() { return active; } };
}
