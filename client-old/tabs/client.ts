// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { LocusEvent } from './types';

/** Typed client interface for tabs plugin */
export interface TabsClient {
  /**  */
  close(index: number, session?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  create(cwd?: string | null, layout?: string | null, name?: string | null, session?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  focus(index: number, session?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  list(session?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  rename(index: number, name: string, session?: string | null): AsyncGenerator<LocusEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
}

/** Typed client implementation for tabs plugin */
class TabsClientImpl implements TabsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *close(index: number, session?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('tabs.close', { index, session });
    yield* extractData<LocusEvent>(stream);
  }

  async *create(cwd?: string | null, layout?: string | null, name?: string | null, session?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('tabs.create', { cwd, layout, name, session });
    yield* extractData<LocusEvent>(stream);
  }

  async *focus(index: number, session?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('tabs.focus', { index, session });
    yield* extractData<LocusEvent>(stream);
  }

  async *list(session?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('tabs.list', { session });
    yield* extractData<LocusEvent>(stream);
  }

  async *rename(index: number, name: string, session?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('tabs.rename', { index, name, session });
    yield* extractData<LocusEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('tabs.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed tabs client from an RPC client */
export function createTabsClient(rpc: RpcClient): TabsClient {
  return new TabsClientImpl(rpc);
}