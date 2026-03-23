// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { RecordingEvent } from './types';

/** Typed client interface for recording plugin */
export interface RecordingClient {
  /**  */
  list(): AsyncGenerator<RecordingEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /**  */
  snapshotLayout(recordingId?: string | null): AsyncGenerator<RecordingEvent>;
  /**  */
  start(outputDir?: string | null, session?: string | null): AsyncGenerator<RecordingEvent>;
  /**  */
  status(): AsyncGenerator<RecordingEvent>;
  /**  */
  stop(recordingId?: string | null): AsyncGenerator<RecordingEvent>;
}

/** Typed client implementation for recording plugin */
class RecordingClientImpl implements RecordingClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *list(): AsyncGenerator<RecordingEvent> {
    const stream = this.rpc.call('recording.list', {});
    yield* extractData<RecordingEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('recording.schema', {});
    return collectOne<unknown>(stream);
  }

  async *snapshotLayout(recordingId?: string | null): AsyncGenerator<RecordingEvent> {
    const stream = this.rpc.call('recording.snapshot_layout', { recording_id: recordingId });
    yield* extractData<RecordingEvent>(stream);
  }

  async *start(outputDir?: string | null, session?: string | null): AsyncGenerator<RecordingEvent> {
    const stream = this.rpc.call('recording.start', { output_dir: outputDir, session });
    yield* extractData<RecordingEvent>(stream);
  }

  async *status(): AsyncGenerator<RecordingEvent> {
    const stream = this.rpc.call('recording.status', {});
    yield* extractData<RecordingEvent>(stream);
  }

  async *stop(recordingId?: string | null): AsyncGenerator<RecordingEvent> {
    const stream = this.rpc.call('recording.stop', { recording_id: recordingId });
    yield* extractData<RecordingEvent>(stream);
  }
}

/** Create a typed recording client from an RPC client */
export function createRecordingClient(rpc: RpcClient): RecordingClient {
  return new RecordingClientImpl(rpc);
}