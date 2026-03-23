var __defProp = Object.defineProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// types.ts
function isPlexusStreamItemData(e) {
  return e.type === "data";
}
function isPlexusStreamItemProgress(e) {
  return e.type === "progress";
}
function isPlexusStreamItemError(e) {
  return e.type === "error";
}
function isPlexusStreamItemDone(e) {
  return e.type === "done";
}
function isPlexusStreamItemRequest(e) {
  return e.type === "request";
}
function isStandardRequestConfirm(e) {
  return e.type === "confirm";
}
function isStandardRequestPrompt(e) {
  return e.type === "prompt";
}
function isStandardRequestSelect(e) {
  return e.type === "select";
}
function isStandardResponseConfirmed(e) {
  return e.type === "confirmed";
}
function isStandardResponseText(e) {
  return e.type === "text";
}
function isStandardResponseSelected(e) {
  return e.type === "selected";
}
function isStandardResponseCancelled(e) {
  return e.type === "cancelled";
}
var PlexusError = class extends Error {
  code;
  recoverable;
  metadata;
  constructor(message, code, recoverable = false, metadata) {
    super(message);
    this.name = "PlexusError";
    this.code = code;
    this.recoverable = recoverable;
    this.metadata = metadata;
  }
};

// rpc.ts
function toCamelCase(str) {
  return str.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}
function transformKeys(obj) {
  if (obj === null || obj === void 0) return obj;
  if (typeof obj !== "object") return obj;
  if (Array.isArray(obj)) return obj.map(transformKeys);
  const result = {};
  for (const [key, value] of Object.entries(obj)) {
    const camelKey = toCamelCase(key);
    result[camelKey] = transformKeys(value);
  }
  return result;
}
async function* extractData(stream) {
  for await (const item of stream) {
    switch (item.type) {
      case "data":
        yield transformKeys(item.content);
        break;
      case "error":
        throw new PlexusError(
          item.message,
          item.code,
          item.recoverable,
          item.metadata
        );
      case "progress":
        break;
      case "done":
        return;
    }
  }
}
async function collectOne(stream) {
  for await (const item of stream) {
    switch (item.type) {
      case "data":
        return transformKeys(item.content);
      case "error":
        throw new PlexusError(
          item.message,
          item.code,
          item.recoverable,
          item.metadata
        );
      case "progress":
        break;
      case "done":
        break;
    }
  }
  throw new Error("No data received from method call");
}

// transport.ts
var PlexusRpcClient = class {
  ws = null;
  nextId = 1;
  pendingRequests = /* @__PURE__ */ new Map();
  subscriptions = /* @__PURE__ */ new Map();
  pendingSubscriptionMessages = /* @__PURE__ */ new Map();
  config;
  connectionPromise = null;
  onBidirectionalRequest;
  constructor(config) {
    this.config = {
      backend: config.backend,
      url: config.url,
      connectionTimeout: config.connectionTimeout ?? 5e3,
      debug: config.debug ?? false
    };
    this.onBidirectionalRequest = config.onBidirectionalRequest;
  }
  setBidirectionalHandler(handler) {
    this.onBidirectionalRequest = handler;
  }
  log(...args) {
    if (this.config.debug) console.log("[PlexusRpcClient]", ...args);
  }
  async connect() {
    if (this.ws?.readyState === WebSocket.OPEN) return;
    if (this.connectionPromise) return this.connectionPromise;
    this.connectionPromise = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error(`Connection timeout after ${this.config.connectionTimeout}ms`));
      }, this.config.connectionTimeout);
      this.ws = new WebSocket(this.config.url);
      this.ws.onopen = () => {
        clearTimeout(timeout);
        this.log("Connected to", this.config.url);
        resolve();
      };
      this.ws.onerror = (event) => {
        clearTimeout(timeout);
        this.log("WebSocket error:", event);
        reject(new Error("WebSocket connection failed"));
      };
      this.ws.onclose = (event) => {
        this.log("WebSocket closed:", event.code, event.reason);
        this.handleDisconnect();
      };
      this.ws.onmessage = (event) => {
        this.handleMessage(event.data.toString());
      };
    });
    try {
      await this.connectionPromise;
    } finally {
      this.connectionPromise = null;
    }
  }
  disconnect() {
    if (this.ws) {
      this.ws.close(1e3, "Client disconnect");
      this.ws = null;
    }
    this.handleDisconnect();
  }
  handleDisconnect() {
    for (const [id, pending] of this.pendingRequests) {
      pending.reject(new Error("Connection closed"));
      this.pendingRequests.delete(id);
    }
    for (const [id, sub] of this.subscriptions) {
      sub.done = true;
      if (sub.waiting) {
        sub.waiting(null);
        sub.waiting = null;
      }
      this.subscriptions.delete(id);
    }
  }
  handleMessage(data) {
    this.log("Received:", data);
    let msg;
    try {
      msg = JSON.parse(data);
    } catch {
      this.log("Failed to parse message:", data);
      return;
    }
    const obj = msg;
    if ("method" in obj && !("id" in obj) && obj.params && typeof obj.params.subscription !== "undefined") {
      this.handleNotification(msg);
      return;
    }
    if ("id" in obj) {
      this.handleResponse(msg);
      return;
    }
    this.log("Unknown message format:", msg);
  }
  handleResponse(resp) {
    const pending = this.pendingRequests.get(resp.id);
    if (!pending) {
      this.log("Unknown request id:", resp.id);
      return;
    }
    this.pendingRequests.delete(resp.id);
    if ("error" in resp) {
      pending.reject(new Error(`RPC error ${resp.error.code}: ${resp.error.message}`));
    } else {
      pending.resolve(resp.result);
    }
  }
  handleNotification(notif) {
    const subscriptionId = notif.params.subscription;
    const item = notif.params.result;
    let sub = this.subscriptions.get(subscriptionId);
    if (!sub) {
      if (!this.pendingSubscriptionMessages.has(subscriptionId)) this.pendingSubscriptionMessages.set(subscriptionId, []);
      this.pendingSubscriptionMessages.get(subscriptionId).push(item);
      return;
    }
    if (item.type === "request") {
      this.handleBidirectionalRequest(item);
      return;
    }
    if (item.type === "done" || item.type === "error") sub.done = true;
    if (sub.waiting) {
      const w = sub.waiting;
      sub.waiting = null;
      w(item);
    } else {
      sub.queue.push(item);
    }
    if (sub.done && sub.queue.length === 0) this.subscriptions.delete(subscriptionId);
  }
  async handleBidirectionalRequest(requestItem) {
    const { requestId, requestData, timeoutMs } = requestItem;
    if (!this.onBidirectionalRequest) {
      this.log("No bidirectional handler, auto-cancelling:", requestId);
      await this.sendBidirectionalResponse(requestId, { type: "cancelled" });
      return;
    }
    const timeoutPromise = new Promise((resolve) => setTimeout(() => resolve(void 0), timeoutMs));
    try {
      const response = await Promise.race([this.onBidirectionalRequest(requestData), timeoutPromise]);
      await this.sendBidirectionalResponse(requestId, response ?? { type: "cancelled" });
    } catch (err) {
      this.log("Bidirectional handler error:", err);
      await this.sendBidirectionalResponse(requestId, { type: "cancelled" });
    }
  }
  async sendBidirectionalResponse(requestId, response) {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      this.log("Cannot send response, not connected");
      return;
    }
    const id = this.nextId++;
    this.ws.send(JSON.stringify({ jsonrpc: "2.0", id, method: `${this.config.backend}.respond`, params: { request_id: requestId, response_data: response } }));
  }
  async *call(method, params) {
    await this.connect();
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) throw new Error("Not connected");
    const sub = { queue: [], waiting: null, done: false };
    const id = this.nextId++;
    const request = {
      jsonrpc: "2.0",
      id,
      method: `${this.config.backend}.call`,
      params: { method, params: params ?? {} }
    };
    this.log("Sending:", JSON.stringify(request));
    const subscriptionIdPromise = new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });
    });
    this.ws.send(JSON.stringify(request));
    const subscriptionId = await subscriptionIdPromise;
    this.log("Got subscription ID:", subscriptionId);
    this.subscriptions.set(subscriptionId, sub);
    const pendingMessages = this.pendingSubscriptionMessages.get(subscriptionId);
    if (pendingMessages) {
      this.pendingSubscriptionMessages.delete(subscriptionId);
      for (const msg of pendingMessages) {
        if (msg.type === "done" || msg.type === "error") sub.done = true;
        sub.queue.push(msg);
      }
    }
    try {
      while (true) {
        if (sub.queue.length > 0) {
          const item2 = sub.queue.shift();
          yield item2;
          if (item2.type === "done" || item2.type === "error") return;
          continue;
        }
        if (sub.done) return;
        const item = await new Promise((resolve) => {
          sub.waiting = resolve;
        });
        if (item === null) return;
        yield item;
        if (item.type === "done" || item.type === "error") return;
      }
    } finally {
      this.subscriptions.delete(subscriptionId);
    }
  }
};
function createClient(config) {
  return new PlexusRpcClient(config);
}

// info/index.ts
var info_exports = {};
__export(info_exports, {
  createInfoClient: () => createInfoClient
});

// info/client.ts
var InfoClientImpl = class {
  rpc;
  constructor(rpc) {
    this.rpc = rpc;
  }
  async *layout() {
    const stream = this.rpc.call("info.layout", {});
    yield* extractData(stream);
  }
  async schema() {
    const stream = this.rpc.call("info.schema", {});
    return collectOne(stream);
  }
  async *status() {
    const stream = this.rpc.call("info.status", {});
    yield* extractData(stream);
  }
};
function createInfoClient(rpc) {
  return new InfoClientImpl(rpc);
}

// panes/index.ts
var panes_exports = {};
__export(panes_exports, {
  createPanesClient: () => createPanesClient
});

// panes/client.ts
var PanesClientImpl = class {
  rpc;
  constructor(rpc) {
    this.rpc = rpc;
  }
  async *batch(commands, panes, settleMs) {
    const stream = this.rpc.call("panes.batch", { commands, panes, settle_ms: settleMs });
    yield* extractData(stream);
  }
  async *capture(full, pane) {
    const stream = this.rpc.call("panes.capture", { full, pane });
    yield* extractData(stream);
  }
  async *close(pane) {
    const stream = this.rpc.call("panes.close", { pane });
    yield* extractData(stream);
  }
  async *create(command, cwd, direction, floating, name, session, target) {
    const stream = this.rpc.call("panes.create", { command, cwd, direction, floating, name, session, target });
    yield* extractData(stream);
  }
  async *exec(command, captureLines, cwd, name, pane, timeoutMs, wait) {
    const stream = this.rpc.call("panes.exec", { capture_lines: captureLines, command, cwd, name, pane, timeout_ms: timeoutMs, wait });
    yield* extractData(stream);
  }
  async *focus(direction) {
    const stream = this.rpc.call("panes.focus", { direction });
    yield* extractData(stream);
  }
  async *layout(cols, rows, commands, cwd, names, tab) {
    const stream = this.rpc.call("panes.layout", { cols, commands, cwd, names, rows, tab });
    yield* extractData(stream);
  }
  async *list(session, tab) {
    const stream = this.rpc.call("panes.list", { session, tab });
    yield* extractData(stream);
  }
  async *poll(pane, captureLines) {
    const stream = this.rpc.call("panes.poll", { capture_lines: captureLines, pane });
    yield* extractData(stream);
  }
  async *rename(name, pane) {
    const stream = this.rpc.call("panes.rename", { name, pane });
    yield* extractData(stream);
  }
  async *resize(direction, amount, pane) {
    const stream = this.rpc.call("panes.resize", { amount, direction, pane });
    yield* extractData(stream);
  }
  async *run(command, closeOnExit, cwd, direction, floating, name, session, target) {
    const stream = this.rpc.call("panes.run", { close_on_exit: closeOnExit, command, cwd, direction, floating, name, session, target });
    yield* extractData(stream);
  }
  async schema() {
    const stream = this.rpc.call("panes.schema", {});
    return collectOne(stream);
  }
  async *send(command, pane, settleMs, timeoutMs) {
    const stream = this.rpc.call("panes.send", { command, pane, settle_ms: settleMs, timeout_ms: timeoutMs });
    yield* extractData(stream);
  }
  async *toggleFloating() {
    const stream = this.rpc.call("panes.toggle_floating", {});
    yield* extractData(stream);
  }
  async *toggleFullscreen() {
    const stream = this.rpc.call("panes.toggle_fullscreen", {});
    yield* extractData(stream);
  }
  async *write(chars, pane, session) {
    const stream = this.rpc.call("panes.write", { chars, pane, session });
    yield* extractData(stream);
  }
};
function createPanesClient(rpc) {
  return new PanesClientImpl(rpc);
}

// recording/index.ts
var recording_exports = {};
__export(recording_exports, {
  createRecordingClient: () => createRecordingClient,
  isRecordingEventError: () => isRecordingEventError,
  isRecordingEventOk: () => isRecordingEventOk,
  isRecordingEventRecordingStarted: () => isRecordingEventRecordingStarted,
  isRecordingEventRecordingStatus: () => isRecordingEventRecordingStatus,
  isRecordingEventRecordingStopped: () => isRecordingEventRecordingStopped,
  isRecordingEventRecordings: () => isRecordingEventRecordings
});

// recording/types.ts
function isRecordingEventRecordingStarted(e) {
  return e.type === "recording_started";
}
function isRecordingEventRecordingStopped(e) {
  return e.type === "recording_stopped";
}
function isRecordingEventRecordingStatus(e) {
  return e.type === "recording_status";
}
function isRecordingEventRecordings(e) {
  return e.type === "recordings";
}
function isRecordingEventOk(e) {
  return e.type === "ok";
}
function isRecordingEventError(e) {
  return e.type === "error";
}

// recording/client.ts
var RecordingClientImpl = class {
  rpc;
  constructor(rpc) {
    this.rpc = rpc;
  }
  async *list() {
    const stream = this.rpc.call("recording.list", {});
    yield* extractData(stream);
  }
  async schema() {
    const stream = this.rpc.call("recording.schema", {});
    return collectOne(stream);
  }
  async *snapshotLayout(recordingId) {
    const stream = this.rpc.call("recording.snapshot_layout", { recording_id: recordingId });
    yield* extractData(stream);
  }
  async *start(outputDir, session) {
    const stream = this.rpc.call("recording.start", { output_dir: outputDir, session });
    yield* extractData(stream);
  }
  async *status() {
    const stream = this.rpc.call("recording.status", {});
    yield* extractData(stream);
  }
  async *stop(recordingId) {
    const stream = this.rpc.call("recording.stop", { recording_id: recordingId });
    yield* extractData(stream);
  }
};
function createRecordingClient(rpc) {
  return new RecordingClientImpl(rpc);
}

// render/index.ts
var render_exports = {};
__export(render_exports, {
  createRenderClient: () => createRenderClient,
  isRenderEventError: () => isRenderEventError,
  isRenderEventPreviewFrame: () => isRenderEventPreviewFrame,
  isRenderEventRecordingInfo: () => isRenderEventRecordingInfo,
  isRenderEventRenderComplete: () => isRenderEventRenderComplete,
  isRenderEventRenderProgress: () => isRenderEventRenderProgress
});

// render/types.ts
function isRenderEventRenderProgress(e) {
  return e.type === "render_progress";
}
function isRenderEventRenderComplete(e) {
  return e.type === "render_complete";
}
function isRenderEventPreviewFrame(e) {
  return e.type === "preview_frame";
}
function isRenderEventRecordingInfo(e) {
  return e.type === "recording_info";
}
function isRenderEventError(e) {
  return e.type === "error";
}

// render/client.ts
var RenderClientImpl = class {
  rpc;
  constructor(rpc) {
    this.rpc = rpc;
  }
  async *info(recordingDir, recordingId) {
    const stream = this.rpc.call("render.info", { recording_dir: recordingDir, recording_id: recordingId });
    yield* extractData(stream);
  }
  async *preview(recordingDir, recordingId, time) {
    const stream = this.rpc.call("render.preview", { recording_dir: recordingDir, recording_id: recordingId, time });
    yield* extractData(stream);
  }
  async *render(borderStyle, fps, idleTimeLimit, outputPath, recordingDir, recordingId) {
    const stream = this.rpc.call("render.render", { border_style: borderStyle, fps, idle_time_limit: idleTimeLimit, output_path: outputPath, recording_dir: recordingDir, recording_id: recordingId });
    yield* extractData(stream);
  }
  async schema() {
    const stream = this.rpc.call("render.schema", {});
    return collectOne(stream);
  }
};
function createRenderClient(rpc) {
  return new RenderClientImpl(rpc);
}

// sessions/index.ts
var sessions_exports = {};
__export(sessions_exports, {
  createSessionsClient: () => createSessionsClient
});

// sessions/client.ts
var SessionsClientImpl = class {
  rpc;
  constructor(rpc) {
    this.rpc = rpc;
  }
  async *create(name, cwd, layout) {
    const stream = this.rpc.call("sessions.create", { cwd, layout, name });
    yield* extractData(stream);
  }
  async *kill(name) {
    const stream = this.rpc.call("sessions.kill", { name });
    yield* extractData(stream);
  }
  async *list() {
    const stream = this.rpc.call("sessions.list", {});
    yield* extractData(stream);
  }
  async schema() {
    const stream = this.rpc.call("sessions.schema", {});
    return collectOne(stream);
  }
};
function createSessionsClient(rpc) {
  return new SessionsClientImpl(rpc);
}

// tabs/index.ts
var tabs_exports = {};
__export(tabs_exports, {
  createTabsClient: () => createTabsClient,
  isLocusEventBatchResult: () => isLocusEventBatchResult,
  isLocusEventCommandExited: () => isLocusEventCommandExited,
  isLocusEventCommandLaunched: () => isLocusEventCommandLaunched,
  isLocusEventCommandStarted: () => isLocusEventCommandStarted,
  isLocusEventCursorPosition: () => isLocusEventCursorPosition,
  isLocusEventDimensions: () => isLocusEventDimensions,
  isLocusEventError: () => isLocusEventError,
  isLocusEventInputSent: () => isLocusEventInputSent,
  isLocusEventLayout: () => isLocusEventLayout,
  isLocusEventLayoutCreated: () => isLocusEventLayoutCreated,
  isLocusEventNoChanges: () => isLocusEventNoChanges,
  isLocusEventOk: () => isLocusEventOk,
  isLocusEventPaneCreated: () => isLocusEventPaneCreated,
  isLocusEventPaneStateInfos: () => isLocusEventPaneStateInfos,
  isLocusEventPanes: () => isLocusEventPanes,
  isLocusEventRegionContent: () => isLocusEventRegionContent,
  isLocusEventScreenCapture: () => isLocusEventScreenCapture,
  isLocusEventScreenChanged: () => isLocusEventScreenChanged,
  isLocusEventScreenContent: () => isLocusEventScreenContent,
  isLocusEventScreenDiff: () => isLocusEventScreenDiff,
  isLocusEventSequenceUpdate: () => isLocusEventSequenceUpdate,
  isLocusEventSessionCreated: () => isLocusEventSessionCreated,
  isLocusEventSessions: () => isLocusEventSessions,
  isLocusEventTabCreated: () => isLocusEventTabCreated,
  isLocusEventTabs: () => isLocusEventTabs,
  isLocusEventTimeout: () => isLocusEventTimeout,
  isLocusEventTrackedPanes: () => isLocusEventTrackedPanes
});

// tabs/types.ts
function isLocusEventSessions(e) {
  return e.type === "sessions";
}
function isLocusEventTabs(e) {
  return e.type === "tabs";
}
function isLocusEventPanes(e) {
  return e.type === "panes";
}
function isLocusEventPaneCreated(e) {
  return e.type === "pane_created";
}
function isLocusEventTabCreated(e) {
  return e.type === "tab_created";
}
function isLocusEventSessionCreated(e) {
  return e.type === "session_created";
}
function isLocusEventScreenCapture(e) {
  return e.type === "screen_capture";
}
function isLocusEventLayout(e) {
  return e.type === "layout";
}
function isLocusEventCommandLaunched(e) {
  return e.type === "command_launched";
}
function isLocusEventCommandStarted(e) {
  return e.type === "command_started";
}
function isLocusEventCommandExited(e) {
  return e.type === "command_exited";
}
function isLocusEventInputSent(e) {
  return e.type === "input_sent";
}
function isLocusEventLayoutCreated(e) {
  return e.type === "layout_created";
}
function isLocusEventBatchResult(e) {
  return e.type === "batch_result";
}
function isLocusEventScreenDiff(e) {
  return e.type === "screen_diff";
}
function isLocusEventScreenContent(e) {
  return e.type === "screen_content";
}
function isLocusEventScreenChanged(e) {
  return e.type === "screen_changed";
}
function isLocusEventCursorPosition(e) {
  return e.type === "cursor_position";
}
function isLocusEventRegionContent(e) {
  return e.type === "region_content";
}
function isLocusEventDimensions(e) {
  return e.type === "dimensions";
}
function isLocusEventNoChanges(e) {
  return e.type === "no_changes";
}
function isLocusEventSequenceUpdate(e) {
  return e.type === "sequence_update";
}
function isLocusEventTrackedPanes(e) {
  return e.type === "tracked_panes";
}
function isLocusEventPaneStateInfos(e) {
  return e.type === "pane_state_infos";
}
function isLocusEventTimeout(e) {
  return e.type === "timeout";
}
function isLocusEventOk(e) {
  return e.type === "ok";
}
function isLocusEventError(e) {
  return e.type === "error";
}

// tabs/client.ts
var TabsClientImpl = class {
  rpc;
  constructor(rpc) {
    this.rpc = rpc;
  }
  async *close(index, session) {
    const stream = this.rpc.call("tabs.close", { index, session });
    yield* extractData(stream);
  }
  async *create(cwd, layout, name, session) {
    const stream = this.rpc.call("tabs.create", { cwd, layout, name, session });
    yield* extractData(stream);
  }
  async *focus(index, session) {
    const stream = this.rpc.call("tabs.focus", { index, session });
    yield* extractData(stream);
  }
  async *list(session) {
    const stream = this.rpc.call("tabs.list", { session });
    yield* extractData(stream);
  }
  async *rename(index, name, session) {
    const stream = this.rpc.call("tabs.rename", { index, name, session });
    yield* extractData(stream);
  }
  async schema() {
    const stream = this.rpc.call("tabs.schema", {});
    return collectOne(stream);
  }
};
function createTabsClient(rpc) {
  return new TabsClientImpl(rpc);
}

// workspace/index.ts
var workspace_exports = {};
__export(workspace_exports, {
  createWorkspaceClient: () => createWorkspaceClient
});

// workspace/client.ts
var WorkspaceClientImpl = class {
  rpc;
  constructor(rpc) {
    this.rpc = rpc;
  }
  async *down(path, workspace) {
    const stream = this.rpc.call("workspace.down", { path, workspace });
    yield* extractData(stream);
  }
  async schema() {
    const stream = this.rpc.call("workspace.schema", {});
    return collectOne(stream);
  }
  async *show(path) {
    const stream = this.rpc.call("workspace.show", { path });
    yield* extractData(stream);
  }
  async *up(path, workspace) {
    const stream = this.rpc.call("workspace.up", { path, workspace });
    yield* extractData(stream);
  }
};
function createWorkspaceClient(rpc) {
  return new WorkspaceClientImpl(rpc);
}
export {
  info_exports as Info,
  panes_exports as Panes,
  PlexusError,
  PlexusRpcClient,
  recording_exports as Recording,
  render_exports as Render,
  sessions_exports as Sessions,
  tabs_exports as Tabs,
  workspace_exports as Workspace,
  collectOne,
  createClient,
  extractData,
  isPlexusStreamItemData,
  isPlexusStreamItemDone,
  isPlexusStreamItemError,
  isPlexusStreamItemProgress,
  isPlexusStreamItemRequest,
  isStandardRequestConfirm,
  isStandardRequestPrompt,
  isStandardRequestSelect,
  isStandardResponseCancelled,
  isStandardResponseConfirmed,
  isStandardResponseSelected,
  isStandardResponseText
};
