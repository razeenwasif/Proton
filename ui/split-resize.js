/** @param {Document} root @param {() => void} changed */
export function createSplitResize(root, changed) {
  const minimum = 120;
  /** @type {HTMLElement | null} */
  let dragging = null;
  /** @param {HTMLElement} gutter */
  function geometry(gutter) {
    const before = gutter.previousElementSibling, after = gutter.nextElementSibling;
    if (!(before instanceof HTMLElement) || !(after instanceof HTMLElement)) return null;
    const stacked = gutter.parentElement?.classList.contains('stacked') ?? false;
    const a = before.getBoundingClientRect(), b = after.getBoundingClientRect();
    const size = stacked ? a.height : a.width;
    return {before, after, stacked, size, span: size + (stacked ? b.height : b.width), start: stacked ? a.top : a.left};
  }
  /** @param {HTMLElement} gutter */
  function describe(gutter) {
    const g = geometry(gutter); if (!g) return;
    gutter.tabIndex = 0; gutter.setAttribute('role', 'separator');
    gutter.setAttribute('aria-orientation', g.stacked ? 'horizontal' : 'vertical');
    gutter.setAttribute('aria-label', `Resize panes ${gutter.dataset.sp} and ${Number(gutter.dataset.sp) + 1}`);
    gutter.setAttribute('aria-valuemin', String(Math.min(50, Math.ceil(minimum / g.span * 100))));
    gutter.setAttribute('aria-valuemax', String(Math.max(50, Math.floor((g.span - minimum) / g.span * 100))));
    gutter.setAttribute('aria-valuenow', String(Math.round(g.size / g.span * 100) || 50));
    gutter.setAttribute('aria-valuetext', `${Math.round(g.size / g.span * 100) || 50}% to the ${g.stacked ? 'top' : 'left'} pane`);
    gutter.title = 'Arrow keys resize · Shift for larger steps · Home resets · Escape returns to editor';
  }
  /** @param {HTMLElement} gutter @param {number} size */
  function resize(gutter, size) {
    const g = geometry(gutter); if (!g || g.span <= minimum * 2) return;
    const total = (parseFloat(g.before.style.getPropertyValue('--share')) || 1) + (parseFloat(g.after.style.getPropertyValue('--share')) || 1);
    const share = Math.max(minimum, Math.min(size, g.span - minimum)) / g.span * total;
    g.before.style.setProperty('--share', String(share)); g.after.style.setProperty('--share', String(total - share));
    describe(gutter);
  }
  /** @param {HTMLElement} gutter */
  function reset(gutter) {
    const g = geometry(gutter); if (!g) return;
    const total = (parseFloat(g.before.style.getPropertyValue('--share')) || 1) + (parseFloat(g.after.style.getPropertyValue('--share')) || 1);
    g.before.style.setProperty('--share', String(total / 2)); g.after.style.setProperty('--share', String(total / 2));
    describe(gutter); changed();
  }
  root.addEventListener('keydown', event => {
    if (!(event.target instanceof HTMLElement) || !event.target.matches('.sp-gutter') || event.metaKey || event.ctrlKey || event.altKey) return;
    const gutter = event.target, g = geometry(gutter); if (!g) return;
    if (event.key === 'Tab') { event.stopPropagation(); return; }
    event.preventDefault(); event.stopPropagation();
    if (event.key === 'Home') reset(gutter);
    else if (event.key === 'Escape') {
      const editor = gutter.parentElement?.querySelector('.split.on [data-editor], .split.on [data-cells]');
      if (editor instanceof HTMLElement) editor.focus();
    } else {
      const direction = g.stacked ? {ArrowUp: -1, ArrowDown: 1} : {ArrowLeft: -1, ArrowRight: 1};
      const step = direction[/** @type {keyof typeof direction} */ (event.key)];
      if (step) { resize(gutter, g.size + step * (event.shiftKey ? 50 : 20)); changed(); }
    }
  });
  root.addEventListener('pointerdown', event => {
    const gutter = event.target instanceof Element && event.target.closest('.sp-gutter');
    if (!(gutter instanceof HTMLElement) || event.button !== 0) return;
    event.preventDefault(); dragging = gutter; gutter.focus(); gutter.classList.add('dragging');
    root.body.style.cursor = geometry(gutter)?.stacked ? 'row-resize' : 'col-resize';
  });
  root.defaultView?.addEventListener('pointermove', event => {
    if (!dragging) return;
    const g = geometry(dragging); if (g) resize(dragging, (g.stacked ? event.clientY : event.clientX) - g.start);
  });
  const finish = () => {
    if (!dragging) return;
    dragging.classList.remove('dragging'); dragging = null; root.body.style.cursor = ''; changed();
  };
  root.defaultView?.addEventListener('pointerup', finish);
  root.defaultView?.addEventListener('pointercancel', finish);
  root.addEventListener('dblclick', event => {
    const gutter = event.target instanceof Element && event.target.closest('.sp-gutter');
    if (gutter instanceof HTMLElement) reset(gutter);
  });
  return {refresh() { root.querySelectorAll('.workspace.on .sp-gutter').forEach(gutter => { if (gutter instanceof HTMLElement) describe(gutter); }); }};
}
