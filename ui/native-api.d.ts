import type {EventBatch, WireCommand} from './protocol';

export interface ForwardedTunnel {
  localPort: number;
  remotePort: number;
  auto: boolean;
}

export interface RemoteStatus {
  connected: boolean;
  host: string | null;
  folder: string | null;
  remoteHostname: string | null;
  remoteHome: string | null;
  error: string | null;
  forwardedPorts: ForwardedTunnel[];
}

export interface SshHostEntry {
  alias: string;
  hostName: string | null;
  user: string | null;
  port: number | null;
}

export interface RecentRemote {
  host: string;
  folder: string;
}

export interface RemoteCatalog {
  hosts: SshHostEntry[];
  recents: RecentRemote[];
}

export interface RemoteDirEntry {
  name: string;
  path: string;
  isRepo: boolean;
  hasDevcontainer: boolean;
}

export interface FullscreenState {
  active: boolean;
  notchHeight: number;
  notchWidth: number;
  notchLeft: number;
}

/** The subset of Tauri's injected API used by Proton. */
export interface NativeApi {
  core: {
    invoke(name: 'dispatch', args: {command: WireCommand}): Promise<EventBatch>;
    invoke(name: 'read_document', args: {path: string}): Promise<ArrayBuffer | number[]>;
    invoke(name: 'save_document_as', args: {path: string}): Promise<string | null>;
    invoke(name: 'read_lab', args: {key: string}): Promise<ArrayBuffer | number[]>;
    invoke(name: 'paste_text'): Promise<string | null>;
    invoke(name: 'copy_text', args: {text: string}): Promise<void>;
    invoke(name: 'backdrop'): Promise<string>;
    invoke(name: 'set_backdrop', args: {material: string}): Promise<string>;
    invoke(name: 'set_opaque', args: {color: string | null}): Promise<void>;
    invoke(name: 'quit', args: {save: boolean}): Promise<void>;
    invoke(name: 'remote_status'): Promise<RemoteStatus>;
    invoke(name: 'remote_hosts'): Promise<RemoteCatalog>;
    invoke(name: 'remote_browse', args: {host: string; path: string | null}): Promise<RemoteDirEntry[]>;
    invoke(name: 'remote_connect', args: {host: string; folder: string | null; newWindow: boolean}): Promise<RemoteStatus>;
    invoke(name: 'remote_disconnect'): Promise<RemoteStatus>;
    invoke(name: 'remote_forward_port', args: {localPort: number; remotePort: number | null}): Promise<RemoteStatus>;
    invoke(name: 'fullscreen_state'): Promise<FullscreenState>;
    invoke(name: 'toggle_fullscreen'): Promise<FullscreenState>;
  };
  event?: {
    listen(name: `proton://events/${string}`, listener: (event: {payload: EventBatch}) => unknown): Promise<() => void>;
    listen(name: `proton://remote-status/${string}`, listener: (event: {payload: RemoteStatus}) => unknown): Promise<() => void>;
    listen(name: `proton://open-remote/${string}`, listener: (event: {payload: unknown}) => unknown): Promise<() => void>;
    listen(name: `proton://fullscreen/${string}`, listener: (event: {payload: FullscreenState}) => unknown): Promise<() => void>;
  };
  window?: {getCurrentWindow(): {
    label: string;
    close(): void | Promise<void>;
    minimize(): void | Promise<void>;
    toggleMaximize(): void | Promise<void>;
    startDragging(): void | Promise<void>;
  }};
}

declare global { var __TAURI__: NativeApi | undefined; }
