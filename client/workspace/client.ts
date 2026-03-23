// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { LocusEvent } from '../tabs/types';

/** Typed client interface for workspace plugin */
export interface WorkspaceClient {
  /**  */
  down(path?: string | null, workspace?: string | null): AsyncGenerator<LocusEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /**  */
  show(path?: string | null): AsyncGenerator<LocusEvent>;
  /**  */
  up(path?: string | null, workspace?: string | null): AsyncGenerator<LocusEvent>;
}

/** Typed client implementation for workspace plugin */
class WorkspaceClientImpl implements WorkspaceClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *down(path?: string | null, workspace?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('workspace.down', { path, workspace });
    yield* extractData<LocusEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('workspace.schema', {});
    return collectOne<unknown>(stream);
  }

  async *show(path?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('workspace.show', { path });
    yield* extractData<LocusEvent>(stream);
  }

  async *up(path?: string | null, workspace?: string | null): AsyncGenerator<LocusEvent> {
    const stream = this.rpc.call('workspace.up', { path, workspace });
    yield* extractData<LocusEvent>(stream);
  }
}

/** Create a typed workspace client from an RPC client */
export function createWorkspaceClient(rpc: RpcClient): WorkspaceClient {
  return new WorkspaceClientImpl(rpc);
}