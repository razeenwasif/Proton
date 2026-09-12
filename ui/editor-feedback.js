/** @param {Document} root */
export function createEditorFeedback(root) {
  const message = root.createElement('div'); message.className = 'editor-message'; message.hidden = true;
  const text = root.createElement('span'); text.setAttribute('role', 'status'); text.setAttribute('aria-live', 'polite');
  const dismiss = root.createElement('button'); dismiss.type = 'button'; dismiss.textContent = 'Dismiss';
  dismiss.addEventListener('click', () => {
    const hadFocus = root.activeElement === dismiss;
    message.hidden = true;
    const editor = root.querySelector('.workspace.on .split.on [data-editor]');
    if (hadFocus && editor instanceof HTMLElement) editor.focus();
  });
  message.append(text, dismiss); root.querySelector('.app')?.append(message);
  const loading = root.createElement('div'); loading.className = 'editor-loading'; loading.hidden = true;
  loading.setAttribute('role', 'status'); root.querySelector('#codex-splits')?.before(loading);
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let loadingTimer;
  return {
    /** @param {import('./protocol').WireCommand} command @param {boolean} active */
    command(command, active) {
      if (command.type !== 'openBuffer' && command.type !== 'focusBuffer') return;
      clearTimeout(loadingTimer);
      loading.hidden = true;
      root.querySelector('#codex-splits')?.setAttribute('aria-busy', String(active));
      if (active) {
        loading.textContent = command.type === 'openBuffer' ? `Opening ${command.path.split(/[\\/]/).at(-1)}…` : 'Switching file…';
        message.hidden = true;
        // Fast local opens should not flash a banner or resize the viewport.
        loadingTimer = setTimeout(() => { loading.hidden = false; }, 150);
      }
    },
    /** @param {Extract<import('./protocol').WireEvent, {type: 'notice'}>} event */
    notice(event) {
      if (event.level !== 'error' && event.level !== 'warn') return;
      text.textContent = event.text; message.dataset.level = event.level; message.hidden = false;
    },
    /** @param {number} view @param {string | undefined} name @param {string[]} lines */
    empty(view, name, lines) {
      const editor = root.getElementById(`editor-view-${view}`);
      let hint = editor?.parentElement?.querySelector('.editor-empty');
      if (!editor) return;
      if (!hint) {
        hint = root.createElement('div'); hint.className = 'editor-empty';
        const title = root.createElement('p');
        const open = root.createElement('button'); open.type = 'button'; open.className = 'btn'; open.dataset.findFile = ''; open.textContent = 'Find a file';
        hint.append(title, open); editor.after(hint);
      }
      if (!(hint instanceof HTMLElement)) return;
      hint.hidden = lines.length !== 1 || lines[0] !== '';
      const title = hint.querySelector('p');
      if (title) title.textContent = name === '[No Name]' ? 'Start a new buffer with i, or open a file.' : 'This file is empty. Press i to start writing.';
    },
  };
}
