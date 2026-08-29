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
  } catch (error) {
    tab.error = String(error);
  }
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
    if (
      event.event_type === "NodeChildrenChanged" &&
      tab.expandedKeys.includes(event.path)
    ) {
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
      tab.selectedPath = "";
      tab.selectedNode = null;
      tab.dataDraft = "";
    }
  } catch (error) {
    tab.error = String(error);
  }
}
