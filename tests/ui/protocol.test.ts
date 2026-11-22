import type { Dispatch, WireEvent } from '../../ui/protocol';
import {createTransport, keyCommand} from '../../ui/transport.js';
import {createEditorState} from '../../ui/editor-state.js';
import {readDocument, setMode} from '../../ui/bridge.js';
import type {NativeApi} from '../../ui/native-api';
import {lineHtml} from '../../ui/editor-row.js';

declare const native: NativeApi;
native.core.invoke('read_lab', {key: 'sky/run-1/plot.png'});
// @ts-expect-error Binary reads use a key, not a path.
native.core.invoke('read_lab', {path: 'sky/run-1/plot.png'});
// @ts-expect-error Native dispatch accepts only generated wire commands.
native.core.invoke('dispatch', {command: {type: 'openBuffer'}});
// @ts-expect-error Public bridge functions preserve their parameter types.
readDocument(12);
// @ts-expect-error Modes are the generated mode union.
setMode('unknown');
// @ts-expect-error Row rendering requires explicit pane and diagnostic context.
lineHtml(0, 'text', undefined, [], [], undefined, undefined, {});

declare const dispatch: Dispatch;
dispatch({ type: 'pickerQuery', text: 'src' });
dispatch({ type: 'openBuffer', path: 'src/main.rs' });
dispatch({ type: 'cycleWorkspace', by: -1 });
// @ts-expect-error The query field is text in the Rust command contract.
dispatch({ type: 'pickerQuery', query: 'src' });
// @ts-expect-error A path is required when opening a buffer.
dispatch({ type: 'openBuffer' });
// @ts-expect-error JavaScript bigint cannot be sent through JSON IPC.
dispatch({ type: 'cycleWorkspace', by: 1n });
declare const event: Extract<WireEvent, { type: 'tree' }>;
event.rows.map(row => row.path);
// @ts-expect-error Tree rows name their label "name".
event.rows.map(row => row.label);

const transport = createTransport({send: async () => ({sequence: 1, events: []})});
transport.dispatch(keyCommand({key: 'Escape'}));
// @ts-expect-error A grouped gesture is checked against the same command union.
transport.dispatchMany([{type: 'focusSplit', pane: 1}]);
// @ts-expect-error Event ordering numbers are JSON numbers, not bigint.
transport.receive({sequence: 2n, events: []});
const editor = createEditorState();
editor.update({type: 'cursor', view: 0, line: 0, column: 3, offset: 3, selection: null});
// @ts-expect-error Viewport updates require a view and all generated fields.
editor.update({type: 'viewport', lines: ['incomplete']});
// @ts-expect-error The editor reducer does not accept unrelated events.
editor.update({type: 'notice', text: 'hello', level: 'info'});

dispatch({type: 'containerStart', config: null});
dispatch({type: 'containerInit', template: 'rust'});
dispatch({type: 'containerClone', url: 'https://github.com/org/repo.git', branch: 'main'});
// @ts-expect-error containerInit requires a template id string.
dispatch({type: 'containerInit'});
declare const containerEv: Extract<WireEvent, {type: 'container'}>;
const _containerPhase: string = containerEv.state.phase;
const _portUrls: string[] = containerEv.state.ports.map((p) => p.url);
const _portAuto: boolean[] = containerEv.state.ports.map((p) => p.autoDetected);
void _containerPhase;
void _portUrls;
void _portAuto;
// @ts-expect-error ContainerView exposes suggestedTemplate, not defaultTemplate.
containerEv.state.defaultTemplate;
