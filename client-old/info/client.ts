// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { LocusEvent } from '../tabs/types';

/** Typed client interface for info plugin */
export interface InfoClient {
  /**  */
  layout(): AsyncGenerator<LocusEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /**  */
  status(): AsyncGenerator<LocusEvent>;
}

/** Typed client implementation for info plugin */
class InfoClientImpl implements InfoClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *layout(): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('info.layout', {});
    yield* extractData<LocusEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('info.schema', {});
    return collectOne<unknown>(stream);
  }

  async *status(): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('info.status', {});
    yield* extractData<LocusEvent>(stream);
  }
}

/** Create a typed info client from an RPC client */
export function createInfoClient(rpc: RpcClient): InfoClient {
  return new InfoClientImpl(rpc);
}