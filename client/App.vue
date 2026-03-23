<template>
  <div class="app">
    <header class="header">
      <h1>Locus Terminal Viewer</h1>
      <div class="status">
        <div :class="['indicator', { connected: isConnected }]"></div>
        <span>{{ statusText }}</span>
      </div>
    </header>

    <div v-if="error" class="error">
      {{ error }}
    </div>

    <div v-else-if="!isConnected" class="loading">
      Connecting to RPC server at ws://127.0.0.1:44480...
    </div>

    <div v-else class="content">
      <div class="tabs">
        <div
          v-for="tab in tabs"
          :key="tab.key"
          :class="['tab', { active: currentTab === tab.key }]"
          @click="selectTab(tab)"
        >
          {{ tab.index }}:{{ tab.name }} ({{ tab.panes.length }})
        </div>
      </div>

      <div class="panes">
        <div
          v-for="pane in currentPanes"
          :key="pane.id"
          class="pane"
        >
          <div class="pane-header">
            {{ pane.id }} {{ pane.name || '' }}
          </div>
          <div class="pane-content" v-html="getPaneContent(pane.id)"></div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { createLocusClient, type LocusClient } from './client';

const isConnected = ref(false);
const statusText = ref('Connecting...');
const error = ref('');
const tabs = ref<any[]>([]);
const currentTab = ref<string | null>(null);
const currentPanes = ref<any[]>([]);
const paneContents = ref<Map<string, string>>(new Map());

let client: LocusClient | null = null;
let refreshInterval: Timer | null = null;

async function connect() {
  try {
    console.log('Creating RPC client...');
    client = await createLocusClient({
      backend: 'locus',
      url: 'ws://127.0.0.1:44480',
      debug: true,
    });

    console.log('RPC client created successfully');
    isConnected.value = true;
    statusText.value = 'Connected';
    error.value = '';

    await refreshLayout();

    // Refresh every 2 seconds
    refreshInterval = setInterval(refreshLayout, 2000);
  } catch (err: any) {
    console.error('Connection error:', err);
    error.value = `Failed to connect: ${err.message}`;
    statusText.value = 'Connection failed';
    isConnected.value = false;
  }
}

async function refreshLayout() {
  if (!client) return;

  try {
    console.log('Fetching layout...');
    const stream = client.info.layout();

    for await (const event of stream) {
      console.log('Layout event:', event);

      if (event.Layout) {
        processLayout(event.Layout.layout);
      }
    }
  } catch (err) {
    console.error('Failed to refresh layout:', err);
  }
}

function processLayout(layout: any) {
  const sessions = layout.sessions || {};
  const allTabs: any[] = [];

  for (const [sessionName, sessionTabs] of Object.entries(sessions)) {
    for (const [tabKey, tabData] of Object.entries(sessionTabs as any)) {
      allTabs.push({
        key: `${sessionName}:${tabKey}`,
        sessionName,
        tabKey,
        ...(tabData as any),
      });
    }
  }

  tabs.value = allTabs;

  // Auto-select first tab if none selected
  if (!currentTab.value && allTabs.length > 0) {
    selectTab(allTabs[0]);
  } else if (currentTab.value) {
    // Refresh current tab panes
    const current = allTabs.find((t) => t.key === currentTab.value);
    if (current) {
      currentPanes.value = current.panes;
      // Fetch content for all panes
      current.panes.forEach((pane: any) => fetchPaneContent(pane.id));
    }
  }
}

async function selectTab(tab: any) {
  currentTab.value = tab.key;
  currentPanes.value = tab.panes;
  paneContents.value.clear();

  // Fetch content for all panes
  tab.panes.forEach((pane: any) => fetchPaneContent(pane.id));
}

async function fetchPaneContent(paneId: string) {
  if (!client) return;

  try {
    const stream = client.panes.capture(false, paneId);

    for await (const event of stream) {
      if (event.PaneContent) {
        const content = event.PaneContent.content;
        paneContents.value.set(paneId, content.html || content.raw || 'No content');
      }
    }
  } catch (err) {
    console.error(`Failed to fetch pane ${paneId}:`, err);
    paneContents.value.set(paneId, 'Error loading content');
  }
}

function getPaneContent(paneId: string): string {
  return paneContents.value.get(paneId) || 'Loading...';
}

onMounted(() => {
  connect();
});

onUnmounted(() => {
  if (refreshInterval) {
    clearInterval(refreshInterval);
  }
  if (client) {
    client.disconnect();
  }
});
</script>

<style scoped>
.app {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

.header {
  background: #0a0a0a;
  border-bottom: 1px solid #333;
  padding: 8px 16px;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.header h1 {
  margin: 0;
  font-size: 14px;
  font-weight: normal;
  color: #00ff00;
}

.status {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}

.indicator {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #ff0000;
  transition: background 0.3s;
}

.indicator.connected {
  background: #00ff00;
}

.error {
  padding: 32px;
  text-align: center;
  color: #ff6b6b;
}

.loading {
  padding: 32px;
  text-align: center;
  color: #666;
}

.content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.tabs {
  display: flex;
  background: #0a0a0a;
  border-bottom: 1px solid #222;
  overflow-x: auto;
}

.tab {
  padding: 6px 12px;
  border-right: 1px solid #222;
  cursor: pointer;
  color: #666;
  white-space: nowrap;
  font-size: 11px;
  transition: all 0.2s;
}

.tab:hover {
  background: #1a1a1a;
  color: #aaa;
}

.tab.active {
  background: #1a1a1a;
  color: #00ff00;
}

.panes {
  flex: 1;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(400px, 1fr));
  gap: 16px;
  padding: 16px;
  overflow: auto;
}

.pane {
  border: 1px solid #333;
  background: #000;
  display: flex;
  flex-direction: column;
  min-height: 300px;
}

.pane-header {
  background: #0a0a0a;
  padding: 4px 8px;
  border-bottom: 1px solid #333;
  font-size: 10px;
  color: #666;
}

.pane-content {
  flex: 1;
  padding: 8px;
  font-family: 'JetBrains Mono', 'Fira Code', 'Monaco', 'Menlo', monospace;
  font-size: 11px;
  line-height: 1.3;
  white-space: pre-wrap;
  overflow: auto;
}
</style>
