import type {EventBatch, WireCommand} from './protocol';

/** The subset of Tauri's injected API used by Proton. */
export interface NativeApi {
  core: {
    invoke(name: 'dispatch', args: {command: WireCommand}): Promise<EventBatch>;
    invoke(name: 'read_document', args: {path: string}): Promise<ArrayBuffer | number[]>;
    invoke(name: 'read_geometry', args: {key: string}): Promise<ArrayBuffer | number[]>;
    invoke(name: 'backdrop'): Promise<string>;
    invoke(name: 'set_backdrop', args: {material: string}): Promise<string>;
    invoke(name: 'set_opaque', args: {color: string | null}): Promise<void>;
  };
  event?: {
    listen(name: 'proton://events', listener: (event: {payload: EventBatch}) => unknown): Promise<() => void>;
  };
  window?: {getCurrentWindow(): {
    close(): void | Promise<void>;
    minimize(): void | Promise<void>;
    toggleMaximize(): void | Promise<void>;
    startDragging(): void | Promise<void>;
  }};
}

declare global { var __TAURI__: NativeApi | undefined; }
