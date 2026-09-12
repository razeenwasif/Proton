import {createTransport} from '../../ui/transport.js';

export async function orderingChecks(test, assert) {
  const notice = text => ({type: 'notice', text, level: 'info'});
  const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return {promise, resolve}; };
  const tick = () => new Promise(done => setTimeout(done, 0));

  await test('Replies and pushes are applied in core order across delayed channels', async () => {
    const reply = deferred(), seen = [], sent = [];
    const transport = createTransport({send: async command => {
      sent.push(command.type);
      return command.type === 'sync' ? {sequence: 1, events: [notice('sync')]} : reply.promise;
    }});
    transport.onEvents(events => seen.push(...events.map(event => event.text)));
    await transport.dispatch({type: 'sync'});
    const command = transport.dispatch({type: 'openBuffer', path: 'a'}); await tick();
    const push = transport.receive({sequence: 3, events: [notice('newer push')]});
    assert(seen.join() === 'sync', 'push overtook its preceding reply');
    reply.resolve({sequence: 2, events: [notice('older reply')]});
    await Promise.all([command, push]);
    assert(seen.join() === 'sync,older reply,newer push', 'wrong merged order');
    await transport.receive({sequence: 2, events: [notice('duplicate')]});
    assert(seen.length === 3, 'stale reply was replayed');
  });
  await test('Initial sync establishes a baseline after pre-listener events', async () => {
    const seen = [];
    const transport = createTransport({send: async () => ({sequence: 10, events: [notice('snapshot')]})});
    transport.onEvents(events => seen.push(...events.map(event => event.text)));
    const stale = transport.receive({sequence: 8, events: [notice('old project')]});
    const newer = transport.receive({sequence: 11, events: [notice('current project')]});
    await transport.dispatch({type: 'sync'}); await Promise.all([stale, newer]);
    assert(seen.join() === 'snapshot,current project', 'startup replayed stale project events');
  });
  await test('A focus-and-edit gesture stays ahead of subsequent typing', async () => {
    const gate = deferred(), sent = [];
    const transport = createTransport({ordered: false, send: async command => {
      sent.push(command.type);
      if (command.type === 'focusSplit') await gate.promise;
      return [];
    }});
    const gesture = transport.dispatchMany([{type: 'focusSplit', index: 1}, {type: 'placeCursor', line: 0, column: 3}]);
    const typing = transport.dispatch({type: 'key', key: {key: 'i', ctrl: false, alt: false, shift: false, meta: false}});
    await tick();
    assert(sent.join() === 'focusSplit', 'later command raced the focus request');
    gate.resolve(); await Promise.all([gesture, typing]);
    assert(sent.join() === 'focusSplit,placeCursor,key', 'gesture interleaved with typing');
  });
  await test('A failed gesture skips its dependent edit and allows later commands', async () => {
    const sent = [], seen = [];
    const transport = createTransport({ordered: false, send: async command => {
      sent.push(command.type);
      if (command.type === 'focusSplit') throw new Error('intentional transport failure');
      return [];
    }});
    transport.onEvents(events => seen.push(...events));
    await transport.dispatchMany([{type: 'focusSplit', index: 1}, {type: 'perform', action: {verb: 'Undo', object: 'None', count: 1, register: null}}]);
    await transport.dispatch({type: 'sync'});
    assert(sent.join() === 'focusSplit,sync', 'failure applied an edit or stranded the queue');
    assert(seen.some(event => event.type === 'notice' && event.level === 'error'), 'failure was silent');
  });
  await test('Commands wait until the push listener is installed', async () => {
    const ready = deferred(), sent = [];
    const transport = createTransport({ordered: false, ready: ready.promise, send: async command => { sent.push(command.type); return []; }});
    const sync = transport.dispatch({type: 'sync'}); await tick();
    assert(sent.length === 0, 'sync started before listener setup');
    ready.resolve(); await sync;
    assert(sent.join() === 'sync', 'sync did not resume after listener setup');
  });
  await test('A missing pushed batch recovers by snapshot without replaying the edit', async () => {
    const sent = [], seen = [];
    let synced = false;
    const transport = createTransport({gapTimeoutMs: 10, send: async command => {
      sent.push(command.type);
      if (command.type === 'sync') {
        const sequence = synced ? 4 : 1; synced = true;
        return {sequence, events: [notice(sequence === 1 ? 'initial' : 'recovered')]};
      }
      // Sequence 2 represents a lost push; 3 is the edit's reply.
      return {sequence: 3, events: [notice('edit')]};
    }});
    transport.onEvents(events => seen.push(...events));
    await transport.dispatch({type: 'sync'});
    await transport.dispatch({type: 'saveBuffer'});
    assert(sent.join() === 'sync,saveBuffer,sync', 'recovery replayed the uncertain command');
    assert(seen.some(event => event.text === 'recovered'), 'snapshot did not recover state');
    assert(seen.some(event => event.level === 'warn'), 'recovery was silent');
  });
  await test('Commands issued before explicit sync establish their own baseline', async () => {
    const sent = [];
    const transport = createTransport({send: async command => {
      sent.push(command.type);
      return {sequence: sent.length, events: [notice(command.type)]};
    }});
    await transport.dispatch({type: 'openBuffer', path: 'early'});
    assert(sent.join() === 'sync,openBuffer', 'early command stranded startup');
  });
  await test('A failed project switch cancels already queued input before recovery', async () => {
    const gate = deferred(), sent = [];
    let sequence = 0;
    const transport = createTransport({send: async command => {
      sent.push(command.type);
      if (command.type === 'focusWorkspace') { await gate.promise; throw new Error('lost switch reply'); }
      return {sequence: ++sequence, events: [notice(command.type)]};
    }});
    await transport.dispatch({type: 'sync'});
    const switching = transport.dispatch({type: 'focusWorkspace', index: 1});
    const typing = transport.dispatch({type: 'key', key: {key: 'x', ctrl: false, alt: false, shift: false, meta: false}});
    gate.resolve(); await Promise.all([switching, typing]);
    assert(!sent.includes('key'), 'queued typing edited an uncertain project');
    await transport.dispatch({type: 'openBuffer', path: 'confirmed.txt'});
    assert(sent.join() === 'sync,focusWorkspace,sync,openBuffer', 'fresh interaction did not recover with a snapshot');
  });
  await test('A reply that never arrives times out and permits a fresh sync', async () => {
    const sent = [], seen = [], activity = [];
    let sequence = 0;
    const transport = createTransport({replyTimeoutMs: 10, send: async command => {
      sent.push(command.type);
      if (command.type === 'openBuffer') return new Promise(() => {});
      return {sequence: ++sequence, events: [notice('snapshot')]};
    }});
    transport.onEvents(events => seen.push(...events));
    transport.onCommand((command, active) => activity.push([command.type, active]));
    await transport.dispatch({type: 'openBuffer', path: 'stalled.txt'});
    await transport.dispatch({type: 'sync'});
    assert(sent.join() === 'sync,openBuffer,sync', 'timeout replayed the open or blocked recovery');
    assert(seen.some(event => event.level === 'error'), 'timeout was silent');
    assert(activity.some(([type, active]) => type === 'openBuffer' && active === false), 'busy state remained stuck');
  });
}
