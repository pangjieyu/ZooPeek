<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface NodeEvent {
  event_type: string;
  path: string;
  zxid: number;
}

const servers = ref("127.0.0.1:2181");
const connected = ref(false);
const connecting = ref(false);
const sessionState = ref("CLOSED");
const sessionId = ref("");
const errorMsg = ref("");

const currentPath = ref("/");
const children = ref<string[]>([]);
const selectedPath = ref("");
const nodeData = ref("");
const events = ref<string[]>([]);

let unlisteners: UnlistenFn[] = [];

function logEvent(text: string) {
  const time = new Date().toLocaleTimeString();
  events.value.unshift(`[${time}] ${text}`);
  if (events.value.length > 100) events.value.pop();
}

async function doConnect() {
  connecting.value = true;
  errorMsg.value = "";
  sessionState.value = "CONNECTING";
  try {
    const result = await invoke<{ session_id: string }>("connect", {
      servers: servers.value,
    });
    connected.value = true;
    sessionId.value = result.session_id;
    logEvent(`已连接，session id = ${result.session_id}`);
    await refreshChildren();
  } catch (e) {
    errorMsg.value = String(e);
    sessionState.value = "CLOSED";
    logEvent(`连接失败: ${e}`);
  } finally {
    connecting.value = false;
  }
}

async function refreshChildren() {
  // watch_children 返回当前子节点并挂一次性 watcher
  children.value = await invoke<string[]>("watch_children", {
    path: currentPath.value,
  });
}

async function enterDir(name: string) {
  currentPath.value =
    currentPath.value === "/"
      ? `/${name}`
      : `${currentPath.value}/${name}`;
  selectedPath.value = "";
  nodeData.value = "";
  await refreshChildren();
}

async function goParent() {
  if (currentPath.value === "/") return;
  const idx = currentPath.value.lastIndexOf("/");
  currentPath.value = idx === 0 ? "/" : currentPath.value.slice(0, idx);
  selectedPath.value = "";
  nodeData.value = "";
  await refreshChildren();
}

function fullPath(name: string): string {
  return currentPath.value === "/"
    ? `/${name}`
    : `${currentPath.value}/${name}`;
}

async function selectNode(name: string) {
  selectedPath.value = fullPath(name);
  // watch_data 返回数据并挂数据 watcher
  nodeData.value = await invoke<string>("watch_data", {
    path: selectedPath.value,
  });
}

onMounted(async () => {
  unlisteners.push(
    await listen<string>("zk-session-state", (e) => {
      sessionState.value = e.payload;
      logEvent(`会话状态变更: ${e.payload}`);
    }),
    await listen<NodeEvent>("zk-node-event", async (e) => {
      logEvent(
        `节点事件: ${e.payload.event_type} path=${e.payload.path} zxid=${e.payload.zxid}`
      );
      // 一次性 watcher 触发后刷新并重新挂 watch
      if (
        e.payload.event_type === "NodeChildrenChanged" &&
        e.payload.path === currentPath.value
      ) {
        await refreshChildren();
      }
      if (
        e.payload.event_type === "NodeDataChanged" &&
        e.payload.path === selectedPath.value
      ) {
        nodeData.value = await invoke<string>("watch_data", {
          path: selectedPath.value,
        });
      }
    })
  );
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
});
</script>

<template>
  <main class="app">
    <header class="toolbar">
      <span class="logo">🦦 ZooPeek</span>
      <input v-model="servers" :disabled="connected" placeholder="host:2181" />
      <button v-if="!connected" :disabled="connecting" @click="doConnect">
        {{ connecting ? "连接中..." : "连接" }}
      </button>
      <span class="badge" :class="sessionState.toLowerCase()">
        {{ sessionState }}
      </span>
      <span v-if="sessionId" class="session">{{ sessionId }}</span>
    </header>

    <p v-if="errorMsg" class="error">{{ errorMsg }}</p>

    <div v-if="connected" class="body">
      <section class="tree">
        <div class="path-bar">
          <button :disabled="currentPath === '/'" @click="goParent">⬆</button>
          <code>{{ currentPath }}</code>
        </div>
        <ul>
          <li
            v-for="c in children"
            :key="c"
            :class="{ selected: selectedPath === fullPath(c) }"
          >
            <span class="name" @click="selectNode(c)">{{ c }}</span>
            <button class="enter" @click="enterDir(c)">→</button>
          </li>
          <li v-if="children.length === 0" class="empty">（空节点）</li>
        </ul>
      </section>

      <section class="detail">
        <h3>{{ selectedPath || "（点击节点名查看数据）" }}</h3>
        <pre v-if="selectedPath">{{ nodeData || "（无数据）" }}</pre>
      </section>
    </div>

    <section class="events">
      <h4>事件流</h4>
      <ul>
        <li v-for="(e, i) in events" :key="i">{{ e }}</li>
      </ul>
    </section>
  </main>
</template>

<style>
:root {
  color-scheme: dark;
  font-family: -apple-system, "PingFang SC", sans-serif;
}
body {
  margin: 0;
  background: #1e1f24;
  color: #d8d9dd;
}
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  padding: 12px;
  box-sizing: border-box;
  gap: 10px;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
}
.logo {
  font-weight: 700;
  font-size: 18px;
}
.toolbar input {
  flex: 0 0 220px;
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid #444;
  background: #2a2b31;
  color: inherit;
}
.toolbar button,
.path-bar button,
.enter {
  padding: 6px 14px;
  border-radius: 6px;
  border: none;
  background: #4f7cff;
  color: white;
  cursor: pointer;
}
.toolbar button:disabled {
  background: #555;
  cursor: default;
}
.badge {
  padding: 3px 10px;
  border-radius: 10px;
  font-size: 12px;
  background: #555;
}
.badge.syncconnected,
.badge.connectedreadonly {
  background: #2b8a3e;
}
.badge.connecting,
.badge.disconnected {
  background: #e8a33d;
  color: #222;
}
.badge.expired,
.badge.closed,
.badge.authfailed {
  background: #c92a2a;
}
.session {
  font-size: 12px;
  color: #888;
}
.error {
  color: #ff6b6b;
  margin: 0;
}
.body {
  display: flex;
  gap: 10px;
  flex: 1;
  min-height: 0;
}
.tree {
  flex: 0 0 320px;
  background: #26272d;
  border-radius: 8px;
  padding: 8px;
  overflow-y: auto;
}
.path-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}
.path-bar code {
  color: #8ab4f8;
}
.tree ul,
.events ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.tree li {
  display: flex;
  justify-content: space-between;
  padding: 4px 8px;
  border-radius: 4px;
}
.tree li:hover {
  background: #33343b;
}
.tree li.selected {
  background: #39415a;
}
.tree .name {
  cursor: pointer;
  flex: 1;
}
.enter {
  padding: 0 8px;
  font-size: 12px;
  background: #444;
}
.empty {
  color: #777;
}
.detail {
  flex: 1;
  background: #26272d;
  border-radius: 8px;
  padding: 8px 14px;
  overflow: auto;
}
.detail pre {
  white-space: pre-wrap;
  word-break: break-all;
  color: #b5e0a8;
}
.events {
  flex: 0 0 160px;
  background: #26272d;
  border-radius: 8px;
  padding: 8px 14px;
  overflow-y: auto;
}
.events h4 {
  margin: 4px 0;
  color: #999;
}
.events li {
  font-size: 12px;
  font-family: monospace;
  color: #9ecbff;
  padding: 1px 0;
}
</style>
