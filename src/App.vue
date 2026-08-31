<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
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
  formatOperationError,
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
  setPasswordRequester,
  store,
  tabById,
  updateExpandedKeys,
  type AuthType,
  type ConnectionTab,
  type KeyringStatus,
  type NodeEvent,
  type SavedConnection,
  type SessionStateEvent,
} from "./store";

const showNewConnection = ref(false);
const editingConnectionId = ref("");
const modalError = ref("");
const modalInfo = ref("");
const testingConnection = ref(false);
const keyringAvailable = ref(true);
const appError = ref("");
const newConnection = reactive({
  name: "",
  servers: "127.0.0.1:2181",
  authType: "none" as AuthType,
  username: "",
  password: "",
  savePassword: true,
});
const unlisteners: UnlistenFn[] = [];

const authTypeOptions = [
  { label: "无", value: "none" },
  { label: "digest", value: "digest" },
  { label: "SASL DIGEST-MD5", value: "sasl_digest_md5" },
];

const showPasswordPrompt = ref(false);
const passwordPrompt = reactive({
  connectionName: "",
  password: "",
});
interface PasswordPromptRequest {
  connection: SavedConnection;
  resolve: (password: string | null) => void;
}
const passwordPromptQueue: PasswordPromptRequest[] = [];
let activePasswordPrompt: PasswordPromptRequest | null = null;

function showNextPasswordPrompt(): void {
  activePasswordPrompt = passwordPromptQueue.shift() ?? null;
  if (!activePasswordPrompt) {
    showPasswordPrompt.value = false;
    return;
  }
  passwordPrompt.connectionName = activePasswordPrompt.connection.name;
  passwordPrompt.password = "";
  showPasswordPrompt.value = true;
}

function requestConnectionPassword(
  connection: SavedConnection
): Promise<string | null> {
  return new Promise((resolve) => {
    passwordPromptQueue.push({ connection, resolve });
    if (!activePasswordPrompt) showNextPasswordPrompt();
  });
}

function finishPasswordPrompt(password: string | null): void {
  const request = activePasswordPrompt;
  if (!request) return;
  activePasswordPrompt = null;
  showPasswordPrompt.value = false;
  passwordPrompt.password = "";
  request.resolve(password);
  showNextPasswordPrompt();
}

function submitPasswordPrompt(): void {
  if (!passwordPrompt.password) return;
  finishPasswordPrompt(passwordPrompt.password);
}

function resetConnectionForm(): void {
  editingConnectionId.value = "";
  newConnection.name = "";
  newConnection.servers = "127.0.0.1:2181";
  newConnection.authType = "none";
  newConnection.username = "";
  newConnection.password = "";
  newConnection.savePassword = true;
  modalError.value = "";
  modalInfo.value = "";
}

async function openNewConnectionModal(): Promise<void> {
  resetConnectionForm();
  showNewConnection.value = true;
  try {
    const status = await invoke<KeyringStatus>("keyring_status");
    keyringAvailable.value = status.available;
  } catch {
    keyringAvailable.value = false;
  }
}

async function openEditConnectionModal(connection: SavedConnection): Promise<void> {
  resetConnectionForm();
  editingConnectionId.value = connection.id;
  newConnection.name = connection.name;
  newConnection.servers = connection.servers;
  newConnection.authType = connection.auth_type;
  newConnection.username = connection.username;
  newConnection.savePassword = connection.save_password;
  showNewConnection.value = true;
  try {
    const status = await invoke<KeyringStatus>("keyring_status");
    keyringAvailable.value = status.available;
  } catch {
    keyringAvailable.value = false;
  }
}

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
  WaitingForManualReconnect: "等待手动重连",
  ReconnectFailed: "重连失败",
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
  if (
    status === "Expired" ||
    status === "AuthFailed" ||
    status === "ReconnectFailed"
  ) {
    return "error";
  }
  if (status === "WaitingForManualReconnect") return "warning";
  return "default";
}

async function createConnection(): Promise<void> {
  modalError.value = "";
  modalInfo.value = "";
  const name = newConnection.name.trim();
  const servers = newConnection.servers.trim();
  if (!name || !servers) {
    modalError.value = "名称和地址不能为空";
    return;
  }
  const authenticated = newConnection.authType !== "none";
  const username = newConnection.username.trim();
  const existing = editingConnectionId.value
    ? store.savedConnections.find((item) => item.id === editingConnectionId.value)
    : undefined;
  const canReuseSavedPassword = Boolean(
    existing?.save_password &&
    existing.auth_type === newConnection.authType &&
    existing.username === username
  );
  if (
    authenticated &&
    (!username || (!newConnection.password && !canReuseSavedPassword))
  ) {
    modalError.value = canReuseSavedPassword
      ? "用户名不能为空"
      : "用户名和密码不能为空";
    return;
  }
  const connection: SavedConnection = {
    id:
      editingConnectionId.value ||
      (globalThis.crypto?.randomUUID?.() ??
        `connection-${Date.now().toString(36)}`),
    name,
    servers,
    auth_type: newConnection.authType,
    username: authenticated ? username : "",
    save_password: authenticated && newConnection.savePassword,
  };
  const password = authenticated && newConnection.password
    ? newConnection.password
    : undefined;
  try {
    const result = await saveConnection(connection, password);
    if (connection.save_password && !result.password_saved) {
      keyringAvailable.value = false;
    }
    showNewConnection.value = false;
    newConnection.password = "";
    const openTab = tabById(connection.id);
    if (openTab) await disconnectConnection(openTab);
    await openConnection(connection, password);
    resetConnectionForm();
  } catch (error) {
    modalError.value = String(error);
  }
}

async function testNewConnection(): Promise<void> {
  modalError.value = "";
  modalInfo.value = "";
  const servers = newConnection.servers.trim();
  const authenticated = newConnection.authType !== "none";
  const username = newConnection.username.trim();
  if (!servers) {
    modalError.value = "地址不能为空";
    return;
  }
  if (authenticated && (!username || !newConnection.password)) {
    modalError.value = "用户名和密码不能为空";
    return;
  }
  testingConnection.value = true;
  try {
    await invoke("test_connection", {
      servers,
      authType: newConnection.authType,
      username: authenticated ? username : "",
      password: authenticated ? newConnection.password : undefined,
    });
    modalInfo.value = authenticated
      ? "连接已建立；凭证将在访问受限节点时继续验证"
      : "连接测试成功";
  } catch (error) {
    const friendly = formatOperationError(error);
    modalError.value = friendly === String(error)
      ? "无法连接，请检查地址和认证方式"
      : friendly;
  } finally {
    testingConnection.value = false;
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
      tab.error = formatOperationError(error);
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
    nodeModalError.value = formatOperationError(error);
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
    tab.error = formatOperationError(error);
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
  setPasswordRequester(requestConnectionPassword);
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
  setPasswordRequester(null);
  activePasswordPrompt?.resolve(null);
  activePasswordPrompt = null;
  for (const request of passwordPromptQueue.splice(0)) request.resolve(null);
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
          <n-button type="primary" size="small" @click="openNewConnectionModal">
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
                <n-space size="small">
                  <n-button text @click.stop="openEditConnectionModal(connection)">
                    编辑
                  </n-button>
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
                </n-space>
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
      :title="editingConnectionId ? '编辑连接' : '新建连接'"
      :style="{ width: '480px' }"
      @after-leave="newConnection.password = ''"
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
        <n-form-item label="认证类型">
          <n-select
            v-model:value="newConnection.authType"
            :options="authTypeOptions"
          />
        </n-form-item>
        <template v-if="newConnection.authType !== 'none'">
          <n-form-item label="用户名" required>
            <n-input
              v-model:value="newConnection.username"
              autocomplete="username"
              placeholder="用户名"
            />
          </n-form-item>
          <n-form-item label="密码" required>
            <n-input
              v-model:value="newConnection.password"
              type="password"
              show-password-on="click"
              autocomplete="new-password"
              :placeholder="editingConnectionId ? '留空则沿用已保存密码' : '密码'"
            />
          </n-form-item>
          <n-form-item :show-feedback="false">
            <n-checkbox v-model:checked="newConnection.savePassword">
              保存到系统钥匙串
            </n-checkbox>
          </n-form-item>
          <div
            v-if="newConnection.savePassword && !keyringAvailable"
            class="auth-hint"
          >
            当前系统不支持安全存储，密码将不会被保存
          </div>
          <div class="auth-hint">
            {{
              newConnection.authType === "digest"
                ? "digest 认证不加密传输，建议仅在可信网络使用"
                : "该方式已过时，仅在服务端要求时使用"
            }}
          </div>
        </template>
        <n-alert v-if="modalError" type="error">{{ modalError }}</n-alert>
        <n-alert v-if="modalInfo" type="info">{{ modalInfo }}</n-alert>
      </n-form>
      <template #footer>
        <n-space justify="space-between">
          <n-button :loading="testingConnection" @click="testNewConnection">
            测试连接
          </n-button>
          <n-space>
            <n-button @click="showNewConnection = false">取消</n-button>
            <n-button type="primary" @click="createConnection">
              {{ editingConnectionId ? "保存并重连" : "保存并连接" }}
            </n-button>
          </n-space>
        </n-space>
      </template>
    </n-modal>

    <n-modal
      v-model:show="showPasswordPrompt"
      preset="card"
      title="输入连接密码"
      :style="{ width: '420px' }"
      :mask-closable="false"
      :close-on-esc="false"
      :closable="false"
    >
      <n-form label-placement="top" @submit.prevent="submitPasswordPrompt">
        <n-form-item :label="passwordPrompt.connectionName" required>
          <n-input
            v-model:value="passwordPrompt.password"
            type="password"
            show-password-on="click"
            autocomplete="current-password"
            autofocus
            placeholder="密码"
            @keyup.enter="submitPasswordPrompt"
          />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="finishPasswordPrompt(null)">取消</n-button>
          <n-button
            type="primary"
            :disabled="!passwordPrompt.password"
            @click="submitPasswordPrompt"
          >
            连接
          </n-button>
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

.auth-hint {
  margin: 8px 0;
  color: rgba(255, 255, 255, 0.58);
  font-size: 12px;
  line-height: 1.5;
}
</style>
