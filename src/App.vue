<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  darkTheme,
  NAlert,
  NButton,
  NCheckbox,
  NCheckboxGroup,
  NCollapse,
  NCollapseItem,
  NConfigProvider,
  NDropdown,
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
  NSelect,
  NSpace,
  NTabPane,
  NTabs,
  NTag,
  NText,
  NTree,
} from "naive-ui";
import type { DropdownOption, TreeOption } from "naive-ui";
import {
  addAclEntry,
  closeTab,
  createChildNode,
  deleteTreeNode,
  deleteSavedConnection,
  disconnectConnection,
  formatJsonDraft,
  handleNodeEvent,
  handleSessionState,
  loadSavedConnections,
  loadTreeNode,
  listNodeChildren,
  openConnection,
  removeAclEntry,
  saveConnection,
  saveNodeAcl,
  saveNodeData,
  selectNode,
  store,
  tabById,
  updateExpandedKeys,
  type ConnectionTab,
  type NodeEvent,
  type SavedConnection,
  type SessionStateEvent,
} from "./store";

const showNewConnection = ref(false);
const modalError = ref("");
const appError = ref("");
const newConnection = reactive({ name: "", servers: "127.0.0.1:2181" });
const unlisteners: UnlistenFn[] = [];

const treeMenu = reactive({
  show: false,
  x: 0,
  y: 0,
  tabId: "",
  path: "",
});
const treeMenuOptions = ref<DropdownOption[]>([]);
const showNewNode = ref(false);
const creatingNode = ref(false);
const nodeModalError = ref("");
const newNode = reactive({ tabId: "", parentPath: "", name: "", data: "" });
const showDeleteNode = ref(false);
const deletingNode = ref(false);
const deleteTarget = reactive({ tabId: "", path: "", recursive: false });

const aclSchemeOptions = ["world", "auth", "digest", "ip"].map((value) => ({
  label: value,
  value,
}));
const aclPermissionOptions = [
  { label: "R", value: 1 },
  { label: "W", value: 2 },
  { label: "C", value: 4 },
  { label: "D", value: 8 },
  { label: "A", value: 16 },
];

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

function treeNodeProps(
  tab: ConnectionTab,
  { option }: { option: TreeOption }
): Record<string, unknown> {
  return {
    onContextmenu: (event: MouseEvent) => {
      event.preventDefault();
      treeMenu.show = false;
      treeMenu.tabId = tab.id;
      treeMenu.path = String(option.key);
      treeMenu.x = event.clientX;
      treeMenu.y = event.clientY;
      treeMenuOptions.value = [
        { label: "新建子节点", key: "create" },
        { label: "删除节点", key: "delete", disabled: option.key === "/" },
      ];
      void nextTick(() => {
        treeMenu.show = true;
      });
    },
  };
}

async function handleTreeMenuSelect(key: string | number): Promise<void> {
  treeMenu.show = false;
  const tab = tabById(treeMenu.tabId);
  if (!tab) return;

  if (key === "create") {
    newNode.tabId = tab.id;
    newNode.parentPath = treeMenu.path;
    newNode.name = "";
    newNode.data = "";
    nodeModalError.value = "";
    showNewNode.value = true;
    return;
  }

  if (key === "delete" && treeMenu.path !== "/") {
    tab.error = "";
    try {
      const children = await listNodeChildren(tab, treeMenu.path);
      deleteTarget.tabId = tab.id;
      deleteTarget.path = treeMenu.path;
      deleteTarget.recursive = children.length > 0;
      showDeleteNode.value = true;
    } catch (error) {
      tab.error = String(error);
    }
  }
}

async function submitNewNode(): Promise<void> {
  const tab = tabById(newNode.tabId);
  if (!tab) return;
  const name = newNode.name.trim();
  if (!name || name.includes("/")) {
    nodeModalError.value = "节点名称不能为空且不能包含 /";
    return;
  }
  creatingNode.value = true;
  nodeModalError.value = "";
  try {
    await createChildNode(tab, newNode.parentPath, name, newNode.data);
    showNewNode.value = false;
  } catch (error) {
    nodeModalError.value = String(error);
  } finally {
    creatingNode.value = false;
  }
}

async function confirmDeleteNode(): Promise<void> {
  const tab = tabById(deleteTarget.tabId);
  if (!tab) return;
  deletingNode.value = true;
  tab.error = "";
  try {
    await deleteTreeNode(
      tab,
      deleteTarget.path,
      deleteTarget.recursive
    );
    showDeleteNode.value = false;
  } catch (error) {
    tab.error = String(error);
    showDeleteNode.value = false;
  } finally {
    deletingNode.value = false;
  }
}

function changeAclScheme(tab: ConnectionTab, scheme: string): void {
  tab.newAcl.scheme = scheme;
  tab.newAcl.id = scheme === "world" ? "anyone" : "";
}

function aclPermissionLabel(permission: number): string {
  return (
    aclPermissionOptions.find((option) => option.value === permission)?.label ??
    "?"
  );
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
                      :node-props="(info) => treeNodeProps(tab, info)"
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

                      <n-collapse class="acl-collapse">
                        <n-collapse-item title="ACL 权限" name="acl">
                          <n-text v-if="tab.aclLoading" depth="3">
                            正在加载 ACL…
                          </n-text>
                          <template v-else>
                            <div v-if="tab.aclDraft.length" class="acl-list">
                              <div
                                v-for="(entry, index) in tab.aclDraft"
                                :key="`${entry.scheme}:${entry.id}:${index}`"
                                class="acl-row"
                              >
                                <n-text code>{{ entry.scheme }}:{{ entry.id }}</n-text>
                                <n-space size="small" class="acl-tags">
                                  <n-tag
                                    v-for="permission in entry.permissions"
                                    :key="permission"
                                    size="small"
                                    type="info"
                                  >
                                    {{ aclPermissionLabel(permission) }}
                                  </n-tag>
                                </n-space>
                                <n-button
                                  text
                                  type="error"
                                  @click="removeAclEntry(tab, index)"
                                >
                                  删除
                                </n-button>
                              </div>
                            </div>
                            <n-empty
                              v-else
                              size="small"
                              description="ACL 列表为空"
                            />

                            <div class="acl-add-form">
                              <n-select
                                :value="tab.newAcl.scheme"
                                :options="aclSchemeOptions"
                                class="acl-scheme"
                                @update:value="(value) => changeAclScheme(tab, value)"
                              />
                              <n-input
                                v-model:value="tab.newAcl.id"
                                placeholder="id"
                              />
                              <n-checkbox-group
                                v-model:value="tab.newAcl.permissions"
                                class="acl-checkboxes"
                              >
                                <n-checkbox
                                  v-for="permission in aclPermissionOptions"
                                  :key="permission.value"
                                  :value="permission.value"
                                  :label="permission.label"
                                />
                              </n-checkbox-group>
                              <n-button @click="addAclEntry(tab)">新增条目</n-button>
                            </div>
                            <div class="acl-save-row">
                              <n-text depth="3">
                                保存会整体替换该节点当前 ACL
                              </n-text>
                              <n-button
                                type="primary"
                                size="small"
                                :loading="tab.aclSaving"
                                @click="saveNodeAcl(tab)"
                              >
                                保存 ACL
                              </n-button>
                            </div>
                          </template>
                        </n-collapse-item>
                      </n-collapse>
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

    <n-dropdown
      trigger="manual"
      placement="bottom-start"
      :show="treeMenu.show"
      :x="treeMenu.x"
      :y="treeMenu.y"
      :options="treeMenuOptions"
      :on-clickoutside="() => (treeMenu.show = false)"
      @select="handleTreeMenuSelect"
    />

    <n-modal
      v-model:show="showNewNode"
      preset="card"
      title="新建子节点"
      :style="{ width: '520px' }"
    >
      <n-form label-placement="top">
        <n-form-item label="父节点">
          <n-text code>{{ newNode.parentPath }}</n-text>
        </n-form-item>
        <n-form-item label="节点名称" required>
          <n-input v-model:value="newNode.name" placeholder="child" />
        </n-form-item>
        <n-form-item label="初始数据">
          <n-input
            v-model:value="newNode.data"
            type="textarea"
            :autosize="{ minRows: 5, maxRows: 12 }"
            placeholder="可留空"
          />
        </n-form-item>
        <n-alert v-if="nodeModalError" type="error">
          {{ nodeModalError }}
        </n-alert>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showNewNode = false">取消</n-button>
          <n-button type="primary" :loading="creatingNode" @click="submitNewNode">
            创建
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal
      v-model:show="showDeleteNode"
      preset="card"
      title="删除节点"
      :style="{ width: '480px' }"
    >
      <n-alert :type="deleteTarget.recursive ? 'warning' : 'info'">
        <template v-if="deleteTarget.recursive">
          将递归删除该节点及全部子节点：{{ deleteTarget.path }}
        </template>
        <template v-else>确认删除节点：{{ deleteTarget.path }}？</template>
      </n-alert>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showDeleteNode = false">取消</n-button>
          <n-button type="error" :loading="deletingNode" @click="confirmDeleteNode">
            删除
          </n-button>
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

.acl-collapse {
  margin-top: 18px;
}

.acl-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.acl-row {
  display: grid;
  grid-template-columns: minmax(140px, auto) 1fr auto;
  align-items: center;
  gap: 10px;
}

.acl-tags {
  min-width: 0;
}

.acl-add-form {
  display: grid;
  grid-template-columns: 110px minmax(140px, 1fr) auto auto;
  align-items: center;
  gap: 10px;
  margin-top: 14px;
}

.acl-checkboxes {
  display: flex;
  gap: 8px;
  white-space: nowrap;
}

.acl-save-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 14px;
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
