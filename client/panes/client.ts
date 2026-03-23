// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { PaneRef } from './types';
import type { LocusEvent } from '../tabs/types';

/** Typed client interface for panes plugin */
export interface PanesClient {
  /**  */
  batch(commands: string[], panes: PaneRef[], settleMs?: number | null): AsyncGenerator<LocusEvent>;
  /**  */
  capture(full?: boolean | null, pane?: PaneRef | null): AsyncGenerator<LocusEvent>;
  /**  */
  close(pane?: PaneRef | null): AsyncGenerator<LocusEvent>;
  /**  */
  create(command?: string | null, cwd?: string | null, direction?: string | null, floating?: boolean | null, name?: string | null, session?: string | null, target?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  exec(command: string, captureLines?: number | null, cwd?: string | null, name?: string | null, pane?: PaneRef | null, timeoutMs?: number | null, wait?: boolean | null): AsyncGenerator<LocusEvent>;
  /**  */
  focus(direction: string): AsyncGenerator<LocusEvent>;
  /**  */
  layout(cols: number, rows: number, commands?: string[] | null, cwd?: string | null, names?: string[] | null, tab?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  list(session?: string | null, tab?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  poll(pane: PaneRef, captureLines?: number | null): AsyncGenerator<LocusEvent>;
  /**  */
  rename(name: string, pane?: PaneRef | null): AsyncGenerator<LocusEvent>;
  /**  */
  resize(direction: string, amount?: number | null, pane?: PaneRef | null): AsyncGenerator<LocusEvent>;
  /**  */
  run(command: string, closeOnExit?: boolean | null, cwd?: string | null, direction?: string | null, floating?: boolean | null, name?: string | null, session?: string | null, target?: string | null): AsyncGenerator<LocusEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /**  */
  send(command: string, pane?: PaneRef | null, settleMs?: number | null, timeoutMs?: number | null): AsyncGenerator<LocusEvent>;
  /**  */
  toggleFloating(): AsyncGenerator<LocusEvent>;
  /**  */
  toggleFullscreen(): AsyncGenerator<LocusEvent>;
  /**  */
  write(chars: string, pane?: PaneRef | null, session?: string | null): AsyncGenerator<LocusEvent>;
}

/** Typed client implementation for panes plugin */
class PanesClientImpl implements PanesClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *batch(commands: string[], panes: PaneRef[], settleMs?: number | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.batch', { commands, panes, settle_ms: settleMs });
    yield* extractData<LocusEvent>(stream);
  }

  async *capture(full?: boolean | null, pane?: PaneRef | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.capture', { full, pane });
    yield* extractData<LocusEvent>(stream);
  }

  async *close(pane?: PaneRef | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.close', { pane });
    yield* extractData<LocusEvent>(stream);
  }

  async *create(command?: string | null, cwd?: string | null, direction?: string | null, floating?: boolean | null, name?: string | null, session?: string | null, target?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.create', { command, cwd, direction, floating, name, session, target });
    yield* extractData<LocusEvent>(stream);
  }

  async *exec(command: string, captureLines?: number | null, cwd?: string | null, name?: string | null, pane?: PaneRef | null, timeoutMs?: number | null, wait?: boolean | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.exec', { capture_lines: captureLines, command, cwd, name, pane, timeout_ms: timeoutMs, wait });
    yield* extractData<LocusEvent>(stream);
  }

  async *focus(direction: string): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.focus', { direction });
    yield* extractData<LocusEvent>(stream);
  }

  async *layout(cols: number, rows: number, commands?: string[] | null, cwd?: string | null, names?: string[] | null, tab?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.layout', { cols, commands, cwd, names, rows, tab });
    yield* extractData<LocusEvent>(stream);
  }

  async *list(session?: string | null, tab?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.list', { session, tab });
    yield* extractData<LocusEvent>(stream);
  }

  async *poll(pane: PaneRef, captureLines?: number | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.poll', { capture_lines: captureLines, pane });
    yield* extractData<LocusEvent>(stream);
  }

  async *rename(name: string, pane?: PaneRef | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.rename', { name, pane });
    yield* extractData<LocusEvent>(stream);
  }

  async *resize(direction: string, amount?: number | null, pane?: PaneRef | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.resize', { amount, direction, pane });
    yield* extractData<LocusEvent>(stream);
  }

  async *run(command: string, closeOnExit?: boolean | null, cwd?: string | null, direction?: string | null, floating?: boolean | null, name?: string | null, session?: string | null, target?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.run', { close_on_exit: closeOnExit, command, cwd, direction, floating, name, session, target });
    yield* extractData<LocusEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('panes.schema', {});
    return collectOne<unknown>(stream);
  }

  async *send(command: string, pane?: PaneRef | null, settleMs?: number | null, timeoutMs?: number | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.send', { command, pane, settle_ms: settleMs, timeout_ms: timeoutMs });
    yield* extractData<LocusEvent>(stream);
  }

  async *toggleFloating(): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.toggle_floating', {});
    yield* extractData<LocusEvent>(stream);
  }

  async *toggleFullscreen(): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.toggle_fullscreen', {});
    yield* extractData<LocusEvent>(stream);
  }

  async *write(chars: string, pane?: PaneRef | null, session?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('panes.write', { chars, pane, session });
    yield* extractData<LocusEvent>(stream);
  }
}

/** Create a typed panes client from an RPC client */
export function createPanesClient(rpc: RpcClient): PanesClient {
  return new PanesClientImpl(rpc);
}