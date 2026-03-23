// Unified client that combines all namespace clients
import { PlexusRpcClient, type PlexusRpcConfig } from './transport';
import { createInfoClient, type InfoClient } from './info';
import { createPanesClient, type PanesClient } from './panes';

export interface LocusClient {
  info: InfoClient;
  panes: PanesClient;
  disconnect: () => void;
}

export async function createLocusClient(config: PlexusRpcConfig): Promise<LocusClient> {
  const transport = new PlexusRpcClient(config);
  await transport.connect();

  return {
    info: createInfoClient(transport),
    panes: createPanesClient(transport),
    disconnect: () => transport.disconnect(),
  };
}
