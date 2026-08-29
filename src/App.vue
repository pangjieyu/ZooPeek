<script setup lang="ts">
import { onMounted, onUnmounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  darkTheme,
  NAlert,
  NButton,
  NConfigProvider,
  NEmpty,
  NForm,
  NFormItem,
  NInput,
  NLayout,
  NLayoutContent,
  NLayoutSider,
  NList,
  NListItem,
  NModal,
  NPopconfirm,
  NScrollbar,
  NSpace,
  NTabPane,
  NTabs,
  NTag,
  NText,
  NTree,
} from "naive-ui";
import {
  closeTab,
  deleteSavedConnection,
  disconnectConnection,
  formatJsonDraft,
  handleNodeEvent,
  handleSessionState,
  loadSavedConnections,
  loadTreeNode,
  openConnection,
  saveConnection,
  saveNodeData,
  selectNode,
  store,
  updateExpandedKeys,
  type NodeEvent,
  type SavedConnection,
  type SessionStateEvent,
} from "./store";

const showNewConnection = ref(false);
const modalError = ref("");
const appError = ref("");
const newConnection = reactive({ name: "", servers: "127.0.0.1:2181" });
const unlisteners: UnlistenFn[] = [];

const statusLabels: Record<string, string> = {
  Connecting: "连接中",
  SyncConnected: "已连接",
  ConnectedReadOnly: "只读连接",
  Disconnected: "已断开",
  Expired: "会话过期",
  AuthFailed: "认证失败",
  Closed: "已关闭",
  SaslAuthenticated: "SASL 已认证",
};

function statusLabel(status: string): string {
  return statusLabels[status] ?? status;
}

function statusType(
  status: string
): "default" | "error" | "info" | "success" | "warning" {
  if (status === "SyncConnected") return "success";
  if (status === "Connecting") return "warning";
  if (status === "ConnectedReadOnly" || status === "SaslAuthenticated") {
    return "info";
  }
  if (status === "Expired" || status === "AuthFailed") return "error";
  return "default";
}

async function createConnection(): Promise<void> {
  modalError.value = "";
  const name = newConnection.name.trim();
  const servers = newConnection.servers.trim();
  if (!name || !servers) {
    modalError.value = "名称和地址不能为空";
    return;
  }
  const connection: SavedConnection = {
    id:
      globalThis.crypto?.randomUUID?.() ??
      `connection-${Date.now().toString(36)}`,
    name,
    servers,
  };
  try {
    await saveConnection(connection);
    showNewConnection.value = false;
    newConnection.name = "";
    await openConnection(connection);
  } catch (error) {
    modalError.value = String(error);
  }
}

async function removeSavedConnection(id: string): Promise<void> {
  try {
    await deleteSavedConnection(id);
  } catch (error) {
    appError.value = String(error);
  }
}

onMounted(async () => {
  try {
    unlisteners.push(
      await listen<SessionStateEvent>("zk-session-state", (event) => {
        void handleSessionState(event.payload);
      }),
      await listen<NodeEvent>("zk-node-event", (event) => {
        void handleNodeEvent(event.payload);
      })
    );
    await loadSavedConnections();
  } catch (error) {
    appError.value = String(error);
  }
});

onUnmounted(() => {
  unlisteners.forEach((unlisten) => unlisten());
});
</script>

<template>
  <n-config-provider :theme="darkTheme">
    <n-layout has-sider class="app-shell">
      <n-layout-sider bordered :width="280" content-style="padding: 16px;">
        <div class="brand-row">
          <div>
            <div class="brand">ZooPeek</div>
            <n-text depth="3">ZooKeeper 客户端</n-text>
          </div>
          <n-button type="primary" size="small" @click="showNewConnection = true">
            新建
          </n-button>
        </div>

        <n-alert v-if="appError" type="error" closable @close="appError = ''">
          {{ appError }}
        </n-alert>

        <div class="section-title">已保存连接</div>
        <n-scrollbar class="connection-list-scroll">
          <n-list v-if="store.savedConnections.length" hoverable clickable>
            <n-list-item
              v-for="connection in store.savedConnections"
              :key="connection.id"
              @click="openConnection(connection)"
            >
              <div class="connection-item">
                <strong>{{ connection.name }}</strong>
                <n-text depth="3">{{ connection.servers }}</n-text>
              </div>
              <template #suffix>
                <n-popconfirm
                  positive-text="删除"
                  negative-text="取消"
                  @positive-click="removeSavedConnection(connection.id)"
                >
                  <template #trigger>
                    <n-button text type="error" @click.stop>删除</n-button>
                  </template>
                  删除这个连接配置？已打开的连接不会关闭。
                </n-popconfirm>
              </template>
            </n-list-item>
          </n-list>
          <n-empty v-else description="还没有保存的连接" />
        </n-scrollbar>
      </n-layout-sider>

      <n-layout-content class="main-content">
        <n-tabs
          v-if="store.tabs.length"
          v-model:value="store.activeTabId"
          type="card"
          closable
          class="connection-tabs"
          @close="(id) => closeTab(String(id))"
        >
          <n-tab-pane
            v-for="tab in store.tabs"
            :key="tab.id"
            :name="tab.id"
            :tab="tab.name"
            display-directive="show"
          >
            <div class="tab-body">
              <div class="status-bar">
                <n-space align="center">
                  <n-tag :type="statusType(tab.status)" round>
                    {{ statusLabel(tab.status) }}
                  </n-tag>
                  <n-text depth="3">{{ tab.servers }}</n-text>
                  <n-text v-if="tab.sessionId" depth="3">
                    Session ID：{{ tab.sessionId }}
                  </n-text>
                </n-space>
                <n-button
                  size="small"
                  secondary
                  type="warning"
                  :disabled="tab.status === 'Disconnected'"
                  @click="disconnectConnection(tab)"
                >
                  断开
                </n-button>
              </div>

              <n-alert
                v-if="tab.error"
                type="error"
                closable
                @close="tab.error = ''"
              >
                {{ tab.error }}
              </n-alert>

              <div class="workspace">
                <section class="tree-pane panel">
                  <div class="panel-title">节点树</div>
                  <n-scrollbar class="tree-scroll">
                    <n-tree
                      block-line
                      show-line
                      :data="tab.tree"
                      :expanded-keys="tab.expandedKeys"
                      :selected-keys="tab.selectedPath ? [tab.selectedPath] : []"
                      :on-load="(option) => loadTreeNode(tab, option)"
                      @update:expanded-keys="
                        (keys) => updateExpandedKeys(tab, keys)
                      "
                      @update:selected-keys="(keys) => selectNode(tab, keys)"
                    />
                  </n-scrollbar>
                </section>

                <section class="detail-pane panel">
                  <div class="panel-title detail-title">
                    <span>节点详情</span>
                    <n-text v-if="tab.selectedNode" code class="detail-path">
                      {{ tab.selectedPath }}
                    </n-text>
                  </div>
                  <n-scrollbar class="detail-scroll">
                    <template v-if="tab.selectedNode">
                      <!-- 元数据压缩为一行小字，主体留给数据编辑器 -->
                      <n-text depth="3" class="meta-line">
                        长度 {{ tab.selectedNode.data_length }} · 版本
                        {{ tab.selectedNode.version }} · 子版本
                        {{ tab.selectedNode.cversion }} · 子节点
                        {{ tab.selectedNode.num_children }} ·
                        {{ tab.selectedNode.is_ephemeral ? "临时节点" : "持久节点" }}
                      </n-text>
                      <n-alert
                        v-if="tab.selectedNode.is_binary"
                        type="warning"
                        class="binary-alert"
                      >
                        该节点为二进制数据，当前为 UTF-8 有损展示，已禁止编辑以防保存损坏原始字节
                      </n-alert>
                      <div class="editor-actions">
                        <n-button
                          size="small"
                          secondary
                          :disabled="tab.selectedNode.is_binary"
                          @click="formatJsonDraft(tab)"
                        >
                          格式化 JSON
                        </n-button>
                        <n-button
                          size="small"
                          type="primary"
                          :loading="tab.saving"
                          :disabled="tab.selectedNode.is_binary"
                          @click="saveNodeData(tab)"
                        >
                          保存数据
                        </n-button>
                      </div>
                      <n-input
                        v-model:value="tab.dataDraft"
                        type="textarea"
                        class="data-editor"
                        :readonly="tab.selectedNode.is_binary"
                        :autosize="{ minRows: 14, maxRows: 26 }"
                        placeholder="节点无数据"
                      />
                    </template>
                    <n-empty v-else description="请选择一个节点查看详情" />
                  </n-scrollbar>
                </section>
              </div>

              <section class="event-pane panel">
                <div class="panel-title">事件流</div>
                <n-scrollbar class="event-scroll">
                  <div v-if="tab.events.length" class="event-list">
                    <div v-for="event in tab.events" :key="event.id" class="event-row">
                      <span class="event-time">{{ event.time }}</span>
                      <span>{{ event.text }}</span>
                    </div>
                  </div>
                  <n-empty v-else size="small" description="暂无事件" />
                </n-scrollbar>
              </section>
            </div>
          </n-tab-pane>
        </n-tabs>

        <div v-else class="welcome">
          <n-empty description="从左侧选择或新建一个 ZooKeeper 连接" />
        </div>
      </n-layout-content>
    </n-layout>

    <n-modal
      v-model:show="showNewConnection"
      preset="card"
      title="新建连接"
      :style="{ width: '480px' }"
    >
      <n-form label-placement="top">
        <n-form-item label="连接名称" required>
          <n-input v-model:value="newConnection.name" placeholder="例如：本地开发环境" />
        </n-form-item>
        <n-form-item label="ZooKeeper 地址" required>
          <n-input
            v-model:value="newConnection.servers"
            placeholder="127.0.0.1:2181"
          />
        </n-form-item>
        <n-alert v-if="modalError" type="error">{{ modalError }}</n-alert>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showNewConnection = false">取消</n-button>
          <n-button type="primary" @click="createConnection">保存并连接</n-button>
        </n-space>
      </template>
    </n-modal>
  </n-config-provider>
</template>

<style>
:root {
  color-scheme: dark;
  font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei",
    sans-serif;
}

html,
body,
#app {
  width: 100%;
  height: 100%;
  margin: 0;
}

body {
  overflow: hidden;
}

.app-shell {
  height: 100vh;
}

.brand-row,
.status-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.brand {
  font-size: 21px;
  font-weight: 700;
  letter-spacing: 0.3px;
}

.section-title,
.panel-title,
.editor-title {
  font-weight: 600;
  color: rgba(255, 255, 255, 0.82);
}

.section-title {
  margin: 24px 0 10px;
  font-size: 13px;
}

.connection-list-scroll {
  max-height: calc(100vh - 135px);
}

.connection-item {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 3px;
}

.connection-item .n-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.main-content,
.connection-tabs {
  height: 100vh;
}

.connection-tabs > .n-tabs-pane-wrapper {
  height: calc(100vh - 46px);
}

.tab-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: calc(100vh - 70px);
  padding: 0 16px 16px;
  box-sizing: border-box;
}

.status-bar {
  flex: none;
  min-height: 34px;
}

.tab-body > .n-alert {
  flex: none;
}

.workspace {
  display: grid;
  flex: 1;
  min-height: 0;
  grid-template-columns: minmax(260px, 35%) minmax(400px, 1fr);
  gap: 12px;
}

.panel {
  min-height: 0;
  padding: 14px;
  border: 1px solid rgba(255, 255, 255, 0.09);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.025);
  box-sizing: border-box;
}

.tree-pane,
.detail-pane,
.event-pane {
  display: flex;
  flex-direction: column;
}

.panel-title {
  margin-bottom: 12px;
  font-size: 14px;
}

.tree-scroll {
  flex: 1;
  min-height: 0;
}

.detail-scroll {
  flex: 1;
  min-height: 0;
}

/* 让树随最宽节点撑开：横向滚动时高亮块覆盖完整宽度 */
.tree-scroll .n-tree {
  width: max-content;
  min-width: 100%;
}

.detail-title {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.detail-path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta-line {
  display: block;
  margin-bottom: 10px;
  font-size: 12px;
}

.data-editor {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.binary-alert {
  margin-bottom: 10px;
}

.editor-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-bottom: 10px;
}

.event-pane {
  flex: none;
  height: 180px;
}

.event-scroll {
  flex: 1;
  min-height: 0;
}

.event-list {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
}

.event-row {
  display: flex;
  gap: 12px;
  padding: 4px 2px;
  line-height: 1.5;
}

.event-time {
  flex: 0 0 auto;
  color: #7e8a9a;
}

.welcome {
  display: grid;
  height: 100%;
  place-items: center;
}
</style>
