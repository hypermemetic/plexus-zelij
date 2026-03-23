// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { LocusEvent } from '../tabs/types';

/** Typed client interface for sessions plugin */
export interface SessionsClient {
  /**  */
  create(name: string, cwd?: string | null, layout?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  kill(name: string): AsyncGenerator<LocusEvent>;
  /**  */
  list(): AsyncGenerator<LocusEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
}

/** Typed client implementation for sessions plugin */
class SessionsClientImpl implements SessionsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *create(name: string, cwd?: string | null, layout?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('sessions.create', { cwd, layout, name });
    yield* extractData<LocusEvent>(stream);
  }

  async *kill(name: string): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('sessions.kill', { name });
    yield* extractData<LocusEvent>(stream);
  }

  async *list(): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('sessions.list', {});
    yield* extractData<LocusEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('sessions.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed sessions client from an RPC client */
export function createSessionsClient(rpc: RpcClient): SessionsClient {
  return new SessionsClientImpl(rpc);
}