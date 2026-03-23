// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { RenderEvent } from './types';

/** Typed client interface for render plugin */
export interface RenderClient {
  /**  */
  info(recordingDir?: string | null, recordingId?: string | null): AsyncGenerator<RenderEvent>;
  /**  */
  preview(recordingDir?: string | null, recordingId?: string | null, time?: number | null): AsyncGenerator<RenderEvent>;
  /**  */
  render(borderStyle?: string | null, fps?: number | null, idleTimeLimit?: number | null, outputPath?: string | null, recordingDir?: string | null, recordingId?: string | null): AsyncGenerator<RenderEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
}

/** Typed client implementation for render plugin */
class RenderClientImpl implements RenderClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *info(recordingDir?: string | null, recordingId?: string | null): AsyncGenerator<RenderEvent> {
    const stream = this.rpc.call('render.info', { recording_dir: recordingDir, recording_id: recordingId });
    yield* extractData<RenderEvent>(stream);
  }

  async *preview(recordingDir?: string | null, recordingId?: string | null, time?: number | null): AsyncGenerator<RenderEvent> {
    const stream = this.rpc.call('render.preview', { recording_dir: recordingDir, recording_id: recordingId, time });
    yield* extractData<RenderEvent>(stream);
  }

  async *render(borderStyle?: string | null, fps?: number | null, idleTimeLimit?: number | null, outputPath?: string | null, recordingDir?: string | null, recordingId?: string | null): AsyncGenerator<RenderEvent> {
    const stream = this.rpc.call('render.render', { border_style: borderStyle, fps, idle_time_limit: idleTimeLimit, output_path: outputPath, recording_dir: recordingDir, recording_id: recordingId });
    yield* extractData<RenderEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('render.schema', {});
    return collectOne<unknown>(stream);
  }
}

/** Create a typed render client from an RPC client */
export function createRenderClient(rpc: RpcClient): RenderClient {
  return new RenderClientImpl(rpc);
}