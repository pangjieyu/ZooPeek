import { invoke } from "@tauri-apps/api/core";
import { reactive } from "vue";
import type { TreeOption } from "naive-ui";

export interface SavedConnection {
  id: string;
  name: string;
  servers: string;
}

export interface NodeDetail {
  data: string;
  data_length: number;
  version: number;
  cversion: number;
  num_children: number;
  is_ephemeral: boolean;
  is_binary: boolean;
}

export interface AclEntry {
  scheme: string;
  id: string;
  perms: number;
}

export interface AclDraftEntry {
  scheme: string;
  id: string;
  permissions: number[];
}

export interface ZkTreeOption extends TreeOption {
  key: string;
  label: string;
  path: string;
  children?: ZkTreeOption[];
}

export interface EventItem {
  id: number;
  time: string;
  text: string;
}

export interface ConnectionTab {
  id: string;
  name: string;
  servers: string;
  status: string;
  sessionId: string;
  error: string;
  tree: ZkTreeOption[];
  expandedKeys: string[];
  selectedPath: string;
  selectedNode: NodeDetail | null;
  dataDraft: string;
  saving: boolean;
  aclDraft: AclDraftEntry[];
  newAcl: AclDraftEntry;
  aclLoading: boolean;
  aclSaving: boolean;
  events: EventItem[];
}

export interface NodeEvent {
  conn_id: string;
  event_type: string;
  path: string;
  zxid: number;
}

export interface SessionStateEvent {
  conn_id: string;
  state: string;
}

export const store = reactive({
  savedConnections: [] as SavedConnection[],
  tabs: [] as ConnectionTab[],
  activeTabId: "",
});

let eventSequence = 0;

export function tabById(connId: string): ConnectionTab | undefined {
  return store.tabs.find((tab) => tab.id === connId);
}

function appendEvent(tab: ConnectionTab, text: string): void {
  tab.events.unshift({
    id: ++eventSequence,
    time: new Date().toLocaleTimeString("zh-CN", { hour12: false }),
    text,
  });
  if (tab.events.length > 200) tab.events.length = 200;
}

function childPath(parentPath: string, name: string): string {
  return parentPath === "/" ? `/${name}` : `${parentPath}/${name}`;
}

function newAclEntry(): AclDraftEntry {
  return {
    scheme: "world",
    id: "anyone",
    permissions: [1, 2, 4, 8, 16],
  };
}

function permissionValues(perms: number): number[] {
  return [1, 2, 4, 8, 16].filter((permission) =>
    Boolean(perms & permission)
  );
}

function findTreeNode(
  nodes: ZkTreeOption[],
  path: string
): ZkTreeOption | undefined {
  for (const node of nodes) {
    if (node.path === path) return node;
    if (node.children) {
      const found = findTreeNode(node.children, path);
      if (found) return found;
    }
  }
  return undefined;
}

function mergeChildren(
  node: ZkTreeOption,
  names: string[]
): ZkTreeOption[] {
  const oldChildren = new Map(
    (node.children ?? []).map((child) => [child.path, child])
  );
  return names.map((name) => {
    const path = childPath(node.path, name);
    return (
      oldChildren.get(path) ?? {
        key: path,
        label: name,
        path,
        isLeaf: false,
      }
    );
  });
}

export async function loadSavedConnections(): Promise<void> {
  store.savedConnections = await invoke<SavedConnection[]>(
    "list_saved_connections"
  );
}

export async function saveConnection(
  connection: SavedConnection
): Promise<void> {
  await invoke("save_connection", { connection });
  await loadSavedConnections();
}

export async function deleteSavedConnection(id: string): Promise<void> {
  await invoke("delete_saved_connection", { id });
  await loadSavedConnections();
}

export async function refreshChildren(
  tab: ConnectionTab,
  path: string
): Promise<void> {
  const names = await invoke<string[]>("watch_children", {
    connId: tab.id,
    path,
  });
  const node = findTreeNode(tab.tree, path);
  if (node) node.children = mergeChildren(node, names);
}

export async function loadTreeNode(
  tab: ConnectionTab,
  option: TreeOption
): Promise<void> {
  await refreshChildren(tab, String(option.key));
}

export function updateExpandedKeys(
  tab: ConnectionTab,
  keys: Array<string | number>
): void {
  tab.expandedKeys = keys.map(String);
}

export async function selectNode(
  tab: ConnectionTab,
  keys: Array<string | number>
): Promise<void> {
  const path = keys.length > 0 ? String(keys[0]) : "";
  tab.selectedPath = path;
  tab.selectedNode = null;
  tab.dataDraft = "";
  tab.aclDraft = [];
  tab.newAcl = newAclEntry();
  tab.aclLoading = Boolean(path);
  tab.error = "";
  if (!path) return;

  try {
    const detail = await invoke<NodeDetail>("watch_data", {
      connId: tab.id,
      path,
    });
    // 快速切换节点时，不让较早的请求覆盖当前详情。
    if (tab.selectedPath === path) {
      tab.selectedNode = detail;
      tab.dataDraft = detail.data;
    }
    if (tab.selectedPath !== path) return;

    const acl = await invoke<AclEntry[]>("get_acl", {
      connId: tab.id,
      path,
    });
    if (tab.selectedPath === path) {
      tab.aclDraft = acl.map((entry) => ({
        scheme: entry.scheme,
        id: entry.id,
        permissions: permissionValues(entry.perms),
      }));
    }
  } catch (error) {
    if (tab.selectedPath !== path) return;
    if (isNoNodeError(error)) {
      // 节点刚被删除（本端或外部），静默取消选中而不是报错
      tab.selectedPath = "";
      tab.selectedNode = null;
      tab.dataDraft = "";
      tab.aclDraft = [];
      tab.newAcl = newAclEntry();
    } else {
      tab.error = String(error);
    }
  } finally {
    if (tab.selectedPath === path) tab.aclLoading = false;
  }
}

export function addAclEntry(tab: ConnectionTab): void {
  const entry = tab.newAcl;
  if (!entry.scheme || (entry.scheme !== "auth" && !entry.id.trim())) {
    tab.error = "ACL 的 scheme 和 id 不能为空";
    return;
  }
  if (entry.permissions.length === 0) {
    tab.error = "请至少选择一项 ACL 权限";
    return;
  }
  tab.aclDraft.push({
    scheme: entry.scheme,
    id: entry.id.trim(),
    permissions: [...entry.permissions],
  });
  tab.newAcl = newAclEntry();
  tab.error = "";
}

export function removeAclEntry(tab: ConnectionTab, index: number): void {
  tab.aclDraft.splice(index, 1);
}

export async function saveNodeAcl(tab: ConnectionTab): Promise<void> {
  if (!tab.selectedPath) return;
  if (tab.aclDraft.length === 0) {
    tab.error = "ACL 列表不能为空";
    return;
  }
  const path = tab.selectedPath;
  tab.aclSaving = true;
  tab.error = "";
  try {
    await invoke("set_acl", {
      connId: tab.id,
      path,
      // set_acl 是整体替换，始终提交界面中的完整列表。
      acl: tab.aclDraft.map((entry) => ({
        scheme: entry.scheme,
        id: entry.id,
        perms: entry.permissions.reduce(
          (sum, permission) => sum | permission,
          0
        ),
      })),
    });
    appendEvent(tab, `已保存 ACL：${path}`);
  } catch (error) {
    tab.error = String(error);
  } finally {
    tab.aclSaving = false;
  }
}

export async function createChildNode(
  tab: ConnectionTab,
  parentPath: string,
  name: string,
  data: string
): Promise<void> {
  await invoke("create_node", {
    connId: tab.id,
    parentPath,
    name,
    data,
  });
  appendEvent(tab, `已新建节点：${childPath(parentPath, name)}`);
}

export async function listNodeChildren(
  tab: ConnectionTab,
  path: string
): Promise<string[]> {
  return invoke<string[]>("list_children", { connId: tab.id, path });
}

function isNoNodeError(error: unknown): boolean {
  return String(error).includes("node not exists");
}

/** 节点被删除后回选其父节点（父节点必然存在，根节点不可删）。 */
async function selectParentAfterDelete(
  tab: ConnectionTab,
  deletedPath: string
): Promise<void> {
  const parentPath = deletedPath.slice(0, deletedPath.lastIndexOf("/")) || "/";
  await selectNode(tab, [parentPath]);
}

export async function deleteTreeNode(
  tab: ConnectionTab,
  path: string,
  recursive: boolean
): Promise<number> {
  // 删除期间 watcher 可能改变 selectedPath，必须在请求前快照回选意图。
  const shouldSelectParent =
    tab.selectedPath === path || tab.selectedPath.startsWith(`${path}/`);
  const deleted = recursive
    ? await invoke<number>("delete_node_recursive", { connId: tab.id, path })
    : await invoke("delete_node", { connId: tab.id, path }).then(() => 1);
  appendEvent(tab, `已删除节点：${path}（共 ${deleted} 个）`);
  if (shouldSelectParent) {
    await selectParentAfterDelete(tab, path);
  }
  return deleted;
}

export async function saveNodeData(tab: ConnectionTab): Promise<void> {
  if (!tab.selectedPath) return;
  // 二进制数据禁止保存：展示层是 UTF-8 有损转换，保存会损坏原始字节。
  if (tab.selectedNode?.is_binary) {
    tab.error = "二进制数据不支持编辑保存";
    return;
  }
  tab.saving = true;
  tab.error = "";
  try {
    await invoke("set_data", {
      connId: tab.id,
      path: tab.selectedPath,
      data: tab.dataDraft,
    });
    appendEvent(tab, `已保存节点数据：${tab.selectedPath}`);
    await selectNode(tab, [tab.selectedPath]);
  } catch (error) {
    tab.error = String(error);
  } finally {
    tab.saving = false;
  }
}

export function formatJsonDraft(tab: ConnectionTab): void {
  try {
    tab.dataDraft = JSON.stringify(JSON.parse(tab.dataDraft), null, 2);
    tab.error = "";
  } catch {
    tab.error = "当前内容不是合法 JSON，无法格式化";
  }
}

export async function openConnection(
  connection: SavedConnection
): Promise<void> {
  let tab = tabById(connection.id);
  if (!tab) {
    tab = reactive<ConnectionTab>({
      id: connection.id,
      name: connection.name,
      servers: connection.servers,
      status: "Disconnected",
      sessionId: "",
      error: "",
      tree: [
        {
          key: "/",
          label: "/",
          path: "/",
          isLeaf: false,
        },
      ],
      expandedKeys: ["/"],
      selectedPath: "",
      selectedNode: null,
      dataDraft: "",
      saving: false,
      aclDraft: [],
      newAcl: newAclEntry(),
      aclLoading: false,
      aclSaving: false,
      events: [],
    });
    store.tabs.push(tab);
  }
  store.activeTabId = tab.id;
  if (
    tab.status === "SyncConnected" ||
    tab.status === "ConnectedReadOnly" ||
    tab.status === "Connecting"
  ) {
    return;
  }

  tab.status = "Connecting";
  tab.error = "";
  tab.name = connection.name;
  tab.servers = connection.servers;
  tab.tree = [
    {
      key: "/",
      label: "/",
      path: "/",
      isLeaf: false,
    },
  ];
  tab.expandedKeys = ["/"];
  tab.selectedPath = "";
  tab.selectedNode = null;
  tab.dataDraft = "";
  tab.aclDraft = [];
  tab.newAcl = newAclEntry();
  tab.aclLoading = false;
  appendEvent(tab, `正在连接 ${connection.servers}`);
  try {
    const result = await invoke<{ session_id: string }>("connect", {
      connId: tab.id,
      servers: tab.servers,
    });
    tab.sessionId = result.session_id;
    appendEvent(tab, `连接成功，session id = ${result.session_id}`);
    await refreshChildren(tab, "/");
  } catch (error) {
    tab.status = "Disconnected";
    tab.error = String(error);
    appendEvent(tab, `连接失败：${String(error)}`);
  }
}

export async function disconnectConnection(tab: ConnectionTab): Promise<void> {
  tab.error = "";
  try {
    await invoke("disconnect", { connId: tab.id });
    appendEvent(tab, "已主动断开连接");
  } catch (error) {
    tab.error = String(error);
  } finally {
    tab.status = "Disconnected";
  }
}

export async function closeTab(connId: string): Promise<void> {
  const tab = tabById(connId);
  if (tab) await disconnectConnection(tab);
  const index = store.tabs.findIndex((item) => item.id === connId);
  if (index >= 0) store.tabs.splice(index, 1);
  if (store.activeTabId === connId) {
    store.activeTabId = store.tabs[Math.max(0, index - 1)]?.id ?? "";
  }
}

export async function handleSessionState(
  event: SessionStateEvent
): Promise<void> {
  const tab = tabById(event.conn_id);
  if (!tab) return;
  tab.status = event.state;
  appendEvent(tab, `会话状态：${event.state}`);
}

export async function handleNodeEvent(event: NodeEvent): Promise<void> {
  const tab = tabById(event.conn_id);
  if (!tab) return;
  appendEvent(
    tab,
    `节点事件：${event.event_type} path=${event.path} zxid=${event.zxid}`
  );

  try {
    if (event.event_type === "NodeChildrenChanged") {
      // ZooKeeper watcher 是一次性的；折叠期间也要刷新并重新注册，避免再次展开时数据陈旧。
      await refreshChildren(tab, event.path);
    }
    if (
      event.event_type === "NodeDataChanged" &&
      tab.selectedPath === event.path
    ) {
      await selectNode(tab, [event.path]);
    }
    if (
      event.event_type === "NodeDeleted" &&
      tab.selectedPath === event.path
    ) {
      // 外部删除当前选中节点：同样回上一级
      await selectParentAfterDelete(tab, event.path);
    }
  } catch (error) {
    tab.error = String(error);
  }
}
