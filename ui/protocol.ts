import type { Command } from "./bindings/Command";
import type { Event } from "./bindings/Event";

// serde_json sends Rust integers as JSON numbers. ts-rs describes i64/u64 as
// bigint, which cannot itself be passed through JSON.stringify. Keep that
// transport adaptation in one place and leave generated bindings untouched.
export type JsonWire<T> = T extends bigint ? number
  : T extends object ? { [K in keyof T]: JsonWire<T[K]> } : T;
export type WireCommand = JsonWire<Command>;
export type WireEvent = JsonWire<Event>;
export type Dispatch = (command: WireCommand) => unknown;
export type EventBatch = { sequence: number; events: WireEvent[] };
export type KeyInput = { key: string; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean; metaKey?: boolean };
