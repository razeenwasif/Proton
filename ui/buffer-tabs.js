/** @typedef {import('./protocol').WireCommand} Command */
/** @typedef {Extract<import('./protocol').WireEvent, {type: 'buffers'}>['names']} Buffers */

/**
 * Manual-activation tabs: arrows move focus, Enter/Space opens, Delete closes,
 * Escape returns to the pane. Stable nodes retain focus as metadata changes.
 * @param {Document} root
 * @param {(commands: Command[]) => Promise<unknown>} dispatchMany
 * @param {(name: string) => string} icon
 */
export function createBufferTabs(root, dispatchMany, icon) {
  /** @type {WeakMap<HTMLElement, Map<string, HTMLElement>>} */
  const entries = new WeakMap();
  /** @type {WeakMap<HTMLElement, string>} */
  const selections = new WeakMap();
  /** @param {HTMLElement} target @param {HTMLElement} tab */
  function rove(target, tab) {
    target.querySelectorAll('[role=tab]').forEach(item => {
      if (item instanceof HTMLElement) item.tabIndex = item === tab ? 0 : -1;
    });
  }
  /** @param {HTMLElement} tab */
  function activate(tab) {
    const pane = Number(tab.closest('[data-view]')?.getAttribute('data-view'));
    return dispatchMany([{type: 'focusSplit', index: pane}, {type: 'focusBuffer', index: Number(tab.dataset.buffer)}]);
  }
  /** @param {HTMLElement} tab */
  function close(tab) {
    return dispatchMany([{type: 'closeBuffer', index: Number(tab.dataset.buffer), force: false}]);
  }
  root.addEventListener('click', event => {
    if (!(event.target instanceof Element)) return;
    const closeButton = event.target.closest('[data-buffer-close]');
    const tab = closeButton?.parentElement?.querySelector('[role=tab]') ?? event.target.closest('[data-buffer]');
    if (!(tab instanceof HTMLElement) || !tab.closest('[data-buffers]')) return;
    event.stopPropagation();
    tab.focus();
    if (closeButton) void close(tab); else void activate(tab);
  });
  root.addEventListener('keydown', event => {
    if (!(event.target instanceof HTMLElement)) return;
    const tab = event.target.closest('[role=tab][data-buffer]');
    const strip = tab?.closest('[data-buffers]');
    if (!(tab instanceof HTMLElement) || !(strip instanceof HTMLElement) || event.metaKey || event.ctrlKey || event.altKey) return;
    const tabs = Array.from(strip.querySelectorAll('[role=tab]')).filter(item => item instanceof HTMLElement);
    const at = tabs.indexOf(tab);
    let next = at;
    if (event.key === 'ArrowRight') next = (at + 1) % tabs.length;
    else if (event.key === 'ArrowLeft') next = (at + tabs.length - 1) % tabs.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = tabs.length - 1;
    else if (event.key === 'Enter' || event.key === ' ') void activate(tab);
    else if (event.key === 'Delete' || event.key === 'Backspace') void close(tab);
    else if (event.key === 'Escape') {
      const pane = tab.closest('[data-view]');
      const editor = pane?.querySelector('[data-editor]');
      if (editor instanceof HTMLElement) {
        void dispatchMany([{type: 'focusSplit', index: Number(pane?.getAttribute('data-view'))}]).then(() => {
          if (root.activeElement === tab && editor.isConnected) editor.focus();
        });
      }
    } else {
      // Ordinary letters belong to the tab, never to the underlying Vim buffer.
      if (event.key !== 'Tab') event.preventDefault();
      event.stopPropagation();
      return;
    }
    event.preventDefault(); event.stopPropagation();
    const chosen = tabs[next];
    if (next !== at && chosen) {
      rove(strip, chosen); chosen.focus(); chosen.scrollIntoView({block: 'nearest', inline: 'nearest'});
    }
  });
  return {
    /** @param {HTMLElement} target @param {Buffers} buffers @param {number} selected @param {number} pane */
    render(target, buffers, selected, pane) {
      target.setAttribute('role', 'tablist');
      target.setAttribute('aria-label', `Files in pane ${pane + 1}`);
      const focused = root.activeElement;
      const ownsFocus = focused instanceof HTMLElement && target.contains(focused);
      const oldIndex = ownsFocus ? Number(focused.closest('[data-buffer]')?.getAttribute('data-buffer') ?? selected) : selected;
      let cache = entries.get(target);
      if (!cache) { cache = new Map(); entries.set(target, cache); }
      const keep = new Set();
      buffers.forEach((buffer, index) => {
        const key = buffer.path ?? `${buffer.name}:${index}`;
        keep.add(key);
        let item = cache.get(key);
        if (!item) {
          item = root.createElement('span'); item.className = 'buf'; item.setAttribute('role', 'presentation');
          const tab = root.createElement('button'); tab.type = 'button'; tab.setAttribute('role', 'tab');
          const image = root.createElement('span'); image.setAttribute('aria-hidden', 'true'); image.innerHTML = icon(buffer.name);
          const label = root.createElement('span'); label.className = 'buffer-name';
          const dot = root.createElement('span'); dot.className = 'mod'; dot.setAttribute('aria-hidden', 'true');
          tab.append(image, label, dot);
          const shut = root.createElement('button'); shut.type = 'button'; shut.className = 'x'; shut.tabIndex = -1; shut.textContent = '×';
          item.append(tab, shut); cache.set(key, item);
        }
        const tab = /** @type {HTMLButtonElement} */ (item.children[0]);
        const shut = /** @type {HTMLButtonElement} */ (item.children[1]);
        tab.dataset.buffer = String(index); tab.id = `buffer-tab-${pane}-${index}`;
        tab.setAttribute('aria-controls', `editor-view-${pane}`);
        tab.setAttribute('aria-selected', String(index === selected));
        tab.setAttribute('aria-label', `${buffer.name}${buffer.modified ? ', unsaved changes' : ''}`);
        tab.title = `${buffer.path ?? buffer.name} — Enter to open · Delete to close`;
        const label = tab.querySelector('.buffer-name');
        if (label && label.textContent !== buffer.name) label.textContent = buffer.name;
        const dot = tab.querySelector('.mod'); if (dot instanceof HTMLElement) dot.hidden = !buffer.modified;
        shut.dataset.bufferClose = String(index); shut.setAttribute('aria-label', `Close ${buffer.name}`); shut.title = `Close ${buffer.name}`;
        item.classList.toggle('on', index === selected);
        if (target.children[index] !== item) target.insertBefore(item, target.children[index] ?? null);
      });
      for (const [key, item] of cache) if (!keep.has(key)) { item.remove(); cache.delete(key); }
      const tabs = Array.from(target.querySelectorAll('[role=tab]')).filter(item => item instanceof HTMLElement);
      const desired = ownsFocus && focused.isConnected ? focused : tabs[Math.min(oldIndex, tabs.length - 1)];
      const active = ownsFocus ? desired : tabs[selected];
      if (active instanceof HTMLElement) rove(target, active);
      if (ownsFocus && !focused.isConnected && desired instanceof HTMLElement) desired.focus();
      const panel = root.getElementById(`editor-view-${pane}`);
      if (tabs[selected]) panel?.setAttribute('aria-labelledby', tabs[selected].id);
      const selectedKey = buffers[selected]?.path ?? `${buffers[selected]?.name}:${selected}`;
      if (!ownsFocus && selections.get(target) !== selectedKey) tabs[selected]?.scrollIntoView({block: 'nearest', inline: 'nearest'});
      selections.set(target, selectedKey);
    },
  };
}
