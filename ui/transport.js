/** @typedef {import('./protocol').WireCommand} Command */
/** @typedef {import('./protocol').WireEvent} Event */
/** @typedef {import('./protocol').EventBatch} Batch */
import {queuedAt, commandTiming} from './performance.js';

/** Commands that can replace the document or pane a viewport describes.
 * @param {Command} command */
export function changesContext(command) {
  return ['sync', 'openWorkspace', 'focusWorkspace', 'closeWorkspace', 'cycleWorkspace',
    'openBuffer', 'focusBuffer', 'closeBuffer', 'split', 'closeSplit', 'onlySplit',
    'focusSplit', 'stepSplit', 'setMode', 'selectPick'].includes(command.type);
}

/**
 * Commands execute in user order. Background pushes and command replies are
 * merged in core order, even if their transport channels overtake each other.
 * The first sync establishes the baseline after events missed before startup.
 * @param {{send: (command: Command) => Promise<Batch | Event[]>, ready?: Promise<unknown>, ordered?: boolean, gapTimeoutMs?: number, replyTimeoutMs?: number}} options
 */
export function createTransport({send, ready = Promise.resolve(), ordered = true, gapTimeoutMs = 2000, replyTimeoutMs = 15000}) {
  /** @type {Set<(events: Event[]) => void>} */
  const listeners = new Set();
  /** @type {Set<(command: Command, active: boolean) => void>} */
  const commandListeners = new Set();
  /** @param {Command} command @param {boolean} active */
  function commandState(command, active) {
    for (const listener of commandListeners) {
      try { listener(command, active); } catch (error) { console.error('proton: command listener failed', error); }
    }
  }
  /** @type {Map<number, Batch>} */
  const pending = new Map();
  /** @type {Map<number, (() => void)[]>} */
  const waiting = new Map();
  /** @type {number | null} */
  let applied = null;
  let queue = Promise.resolve();
  let generation = 0;
  let navigating = 0;

  /** @param {Command} command */
  async function request(command) {
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    let timer;
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('The editor did not respond in time. Try again.')), replyTimeoutMs);
    });
    try {
      return await Promise.race([send(command), /** @type {Promise<never>} */ (timeout)]);
    } finally { clearTimeout(timer); }
  }

  /** @param {Event[]} events */
  function deliver(events) {
    if (!events.length) return;
    for (const listener of listeners) {
      // A broken renderer must not strand the command queue or other listeners.
      try { listener(events); }
      catch (error) { console.error('proton: event listener failed', error); }
    }
  }
  function drain() {
    if (applied === null) return;
    let next;
    while ((next = pending.get(applied + 1))) {
      pending.delete(++applied);
      deliver(next.events);
    }
    for (const [sequence, resolve] of waiting) {
      if (sequence <= applied) {
        waiting.delete(sequence);
        resolve.forEach((done) => done());
      }
    }
  }
  /** @param {Batch | Event[]} batch @param {boolean} [baseline] */
  function receive(batch, baseline = false) {
    if (!ordered) {
      deliver(Array.isArray(batch) ? batch : batch.events);
      return Promise.resolve();
    }
    if (Array.isArray(batch) || !Number.isSafeInteger(batch.sequence)) {
      throw new Error('Incompatible event transport; reload the matching shell and UI');
    }
    if (!batch.events.length) return Promise.resolve();
    if (applied === null && baseline) {
      applied = batch.sequence;
      for (const sequence of pending.keys()) if (sequence <= applied) pending.delete(sequence);
      deliver(batch.events);
    } else if (applied === null || batch.sequence > applied) {
      pending.set(batch.sequence, batch);
    }
    const done = new Promise(resolve => {
      if (applied !== null && batch.sequence <= applied) resolve(undefined);
      else waiting.set(batch.sequence, [...(waiting.get(batch.sequence) ?? []), () => resolve(undefined)]);
    });
    drain();
    return done;
  }
  /**
   * A lost push must not leave all future typing waiting on a missing number.
   * Recover from a fresh snapshot without replaying the uncertain edit.
   * @param {Batch | Event[]} batch @param {boolean} baseline
   */
  async function acceptReply(batch, baseline) {
    const delivered = receive(batch, baseline);
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    let timer;
    const timeout = new Promise(resolve => { timer = setTimeout(() => resolve(false), gapTimeoutMs); });
    try {
      if (await Promise.race([delivered.then(() => true), timeout])) return;
      applied = null;
      await receive(await request({type: 'sync'}), true);
      deliver([{type: 'notice', level: 'warn', text: 'The event stream was interrupted. The editor has been resynced.'}]);
    } finally { clearTimeout(timer); }
  }
  /**
   * Reserve the whole gesture before later keystrokes can enter the queue.
   * A failed focus/select aborts dependent and already queued input. Fresh
   * interaction can recover by snapshot without replaying uncertain edits.
   * @param {Command[]} commands
   */
  function dispatchMany(commands) {
    // Scroll/resize notifications describe the currently painted document.
    // During queued navigation that document is stale. Measure again after
    // navigation settles instead of applying its scroll to the next project.
    if (navigating && commands.every(command => command.type === 'viewport')) return queue;
    const navigation = commands.some(changesContext);
    if (navigation) navigating++;
    const queued = queuedAt();
    const requestedGeneration = generation;
    const job = queue.then(async () => {
      // Input already queued against an uncertain selection must not be
      // applied to whichever buffer happens to survive the failed request.
      if (requestedGeneration !== generation) return;
      await ready;
      if (ordered && applied === null && commands[0]?.type !== 'sync') {
        await receive(await request({type: 'sync'}), true);
      }
      for (const command of commands) {
        const timing = commandTiming(queued, command.type);
        commandState(command, true);
        try {
          const batch = await request(command);
          timing?.reply();
          await acceptReply(batch, command.type === 'sync');
          timing?.done();
        } catch (error) {
          // The core may have applied the request before its reply was lost.
          // Re-establish a snapshot before sending the next user command.
          if (ordered) applied = null;
          generation++;
          console.error('proton: dispatch failed', command.type, error);
          if (command.type === 'openPicker') deliver([{type: 'pickerClosed'}]);
          deliver([{type: 'notice', level: 'error', text: `Could not ${command.type}: ${String(error)} Queued input was cancelled.`}]);
          break;
        } finally { commandState(command, false); }
      }
    });
    // Failure to establish the listener is visible and never becomes an
    // unhandled rejection. Every future request can still report the failure.
    queue = job.catch(error => {
      generation++;
      if (ordered) applied = null;
      deliver([{type: 'notice', level: 'error', text: `Could not connect: ${String(error)} Queued input was cancelled.`}]);
    }).finally(() => { if (navigation) navigating--; });
    return queue;
  }
  return {
    receive, deliver, dispatchMany,
    /** Drop sequence state when an in-place window switches between local & SSH hosts. */
    reset() {
      pending.clear();
      for (const resolvers of waiting.values()) {
        resolvers.forEach((done) => done());
      }
      waiting.clear();
      applied = null;
      generation++;
    },
    /** @param {Command} command */
    dispatch: command => dispatchMany([command]),
    /** @param {(events: Event[]) => void} listener */
    onEvents(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    /** @param {(command: Command, active: boolean) => void} listener */
    onCommand(listener) { commandListeners.add(listener); return () => commandListeners.delete(listener); },
  };
}

/** @param {import('./protocol').KeyInput} event @returns {Command} */
export function keyCommand(event) {
  return {type: 'key', key: {key: event.key, ctrl: !!event.ctrlKey, alt: !!event.altKey,
    shift: !!event.shiftKey, meta: !!event.metaKey}};
}
