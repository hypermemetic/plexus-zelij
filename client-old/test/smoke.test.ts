// Auto-generated smoke test for locus backend
// Run with: bun test

import { test, expect, beforeAll, afterAll } from "bun:test";
import { PlexusRpcClient } from '../transport';
import type { PlexusStreamItem } from "../types";

const WS_URL = process.env.PLEXUS_URL ?? "ws://127.0.0.1:44480";

let client: PlexusRpcClient;

beforeAll(async () => {
  client = new PlexusRpcClient({
    backend: "locus",
    url: WS_URL,
    debug: false,
    connectionTimeout: 5000,
  });
  await client.connect();
}, 10_000);

afterAll(() => {
  client?.disconnect();
});

test("connects to locus backend", () => {
  expect(client).toBeDefined();
});

test("locus.schema returns stream ending in done", async () => {
  const items: PlexusStreamItem[] = [];
  for await (const item of client.call("locus.schema", {})) {
    items.push(item);
    if (item.type === "done") break;
    if (item.type === "error" && !item.recoverable) {
      throw new Error(`Backend error: ${item.message}`);
    }
  }
  expect(items.length).toBeGreaterThan(0);
  expect(items[items.length - 1].type).toBe("done");
}, 10_000);
