use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};
use tauri::{async_runtime::JoinHandle, AppHandle, Emitter, Manager, State};
use zookeeper_client as zk;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum WatchKind {
    Children,
    Data,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct WatchKey {
    conn_id: String,
    path: String,
    kind: WatchKind,
}

/// 连接和 watcher 统一按 conn_id 管理，避免多 tab 之间互相覆盖。
struct ConnectionManager {
    clients: Mutex<HashMap<String, zk::Client>>,
    watchers: Mutex<HashMap<WatchKey, JoinHandle<()>>>,
    session_watchers: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl ConnectionManager {
    fn new() -> Self {
        Self {
            clients: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
            session_watchers: Mutex::new(HashMap::new()),
        }
    }

    fn client(&self, conn_id: &str) -> Result<zk::Client, String> {
        self.clients
            .lock()
            .map_err(|error| error.to_string())?
            .get(conn_id)
            .cloned()
            .ok_or_else(|| format!("连接 {conn_id} 不存在或已断开"))
    }

    fn cancel_tasks(&self, conn_id: &str) -> Result<(), String> {
        let mut watchers = self.watchers.lock().map_err(|error| error.to_string())?;
        let keys = watchers
            .keys()
            .filter(|key| key.conn_id == conn_id)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(task) = watchers.remove(&key) {
                // abort 会 drop 任务内持有的一次性 watcher，从服务端注销 watch。
                task.abort();
            }
        }
        drop(watchers);

        if let Some(task) = self
            .session_watchers
            .lock()
            .map_err(|error| error.to_string())?
            .remove(conn_id)
        {
            task.abort();
        }
        Ok(())
    }

    fn replace_watch(&self, key: WatchKey, task: JoinHandle<()>) -> Result<(), String> {
        if let Some(old_task) = self
            .watchers
            .lock()
            .map_err(|error| error.to_string())?
            .insert(key, task)
        {
            old_task.abort();
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ConnectResult {
    session_id: String,
}

#[derive(Serialize)]
struct NodeResult {
    data: String,
    data_length: i32,
    version: i32,
    cversion: i32,
    num_children: i32,
    is_ephemeral: bool,
    /// 非 UTF-8 数据标记：前端据此禁止编辑，防止 from_utf8_lossy 后保存导致数据损坏。
    is_binary: bool,
}

#[derive(Deserialize, Serialize)]
struct AclEntry {
    scheme: String,
    id: String,
    perms: i32,
}

fn permission_from_bits(bits: i32) -> Result<zk::Permission, String> {
    if !(0..=31).contains(&bits) {
        return Err(format!("无效的 ACL 权限值：{bits}"));
    }

    let mut permission = zk::Permission::NONE;
    for (bit, value) in [
        (1, zk::Permission::READ),
        (2, zk::Permission::WRITE),
        (4, zk::Permission::CREATE),
        (8, zk::Permission::DELETE),
        (16, zk::Permission::ADMIN),
    ] {
        if bits & bit != 0 {
            permission = permission | value;
        }
    }
    Ok(permission)
}

fn permission_bits(permission: zk::Permission) -> i32 {
    [
        (1, zk::Permission::READ),
        (2, zk::Permission::WRITE),
        (4, zk::Permission::CREATE),
        (8, zk::Permission::DELETE),
        (16, zk::Permission::ADMIN),
    ]
    .into_iter()
    .filter_map(|(bit, value)| permission.has(value).then_some(bit))
    .sum()
}

impl NodeResult {
    fn from_data_and_stat(data: Vec<u8>, stat: zk::Stat) -> Self {
        Self {
            is_binary: std::str::from_utf8(&data).is_err(),
            data: String::from_utf8_lossy(&data).into_owned(),
            data_length: stat.data_length,
            version: stat.version,
            cversion: stat.cversion,
            num_children: stat.num_children,
            is_ephemeral: stat.ephemeral_owner != 0,
        }
    }
}

#[derive(Clone, Serialize)]
struct NodeEvent {
    conn_id: String,
    event_type: String,
    path: String,
    zxid: i64,
}

#[derive(Clone, Serialize)]
struct SessionStateEvent {
    conn_id: String,
    state: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedConnection {
    id: String,
    name: String,
    servers: String,
}

#[tauri::command]
async fn connect(
    app: AppHandle,
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    servers: String,
) -> Result<ConnectResult, String> {
    let client = zk::Client::connect(&servers)
        .await
        .map_err(|error| error.to_string())?;
    let session_id = client.session_id().to_string();
    let mut state_watcher = client.state_watcher();

    // 新连接成功后再替换旧连接，连接失败时保留原连接。
    manager.cancel_tasks(&conn_id)?;
    manager
        .clients
        .lock()
        .map_err(|error| error.to_string())?
        .insert(conn_id.clone(), client);

    let current_state = state_watcher.state();
    let _ = app.emit(
        "zk-session-state",
        SessionStateEvent {
            conn_id: conn_id.clone(),
            state: format!("{current_state:?}"),
        },
    );

    let app_handle = app.clone();
    let watcher_conn_id = conn_id.clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let state = state_watcher.changed().await;
            let _ = app_handle.emit(
                "zk-session-state",
                SessionStateEvent {
                    conn_id: watcher_conn_id.clone(),
                    state: format!("{state:?}"),
                },
            );
            if matches!(
                state,
                zk::SessionState::Expired | zk::SessionState::Closed | zk::SessionState::AuthFailed
            ) {
                break;
            }
        }
    });
    if let Some(old_task) = manager
        .session_watchers
        .lock()
        .map_err(|error| error.to_string())?
        .insert(conn_id, task)
    {
        old_task.abort();
    }

    Ok(ConnectResult { session_id })
}

#[tauri::command]
fn disconnect(manager: State<'_, ConnectionManager>, conn_id: String) -> Result<(), String> {
    manager.cancel_tasks(&conn_id)?;
    manager
        .clients
        .lock()
        .map_err(|error| error.to_string())?
        .remove(&conn_id);
    Ok(())
}

#[tauri::command]
async fn list_children(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
) -> Result<Vec<String>, String> {
    let client = manager.client(&conn_id)?;
    let mut children = client
        .list_children(&path)
        .await
        .map_err(|error| error.to_string())?;
    children.sort();
    Ok(children)
}

#[tauri::command]
async fn get_node(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
) -> Result<NodeResult, String> {
    let client = manager.client(&conn_id)?;
    let (data, stat) = client
        .get_data(&path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(NodeResult::from_data_and_stat(data, stat))
}

#[tauri::command]
async fn set_data(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
    data: String,
) -> Result<(), String> {
    manager
        .client(&conn_id)?
        .set_data(&path, data.as_bytes(), None)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn create_node(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    parent_path: String,
    name: String,
    data: String,
) -> Result<(), String> {
    if name.is_empty() || name.contains('/') {
        return Err("节点名称不能为空且不能包含 /".to_string());
    }
    let path = if parent_path == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", parent_path.trim_end_matches('/'))
    };
    let client = manager.client(&conn_id)?;
    // ZooKeeper ACL 不会继承；显式复制父节点 ACL，避免新节点意外变成 world:anyone:ALL。
    let (parent_acl, _) = client
        .get_acl(&parent_path)
        .await
        .map_err(|error| error.to_string())?;
    let options = zk::CreateMode::Persistent.with_acls(zk::Acls::new(&parent_acl));
    client
        .create(&path, data.as_bytes(), &options)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn delete_node(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
) -> Result<(), String> {
    if path == "/" {
        return Err("不能删除根节点".to_string());
    }
    let client = manager.client(&conn_id)?;
    let children = client
        .list_children(&path)
        .await
        .map_err(|error| error.to_string())?;
    if !children.is_empty() {
        return Err("节点存在子节点，不能直接删除".to_string());
    }
    client
        .delete(&path, None)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn delete_node_recursive(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
) -> Result<u32, String> {
    if path == "/" {
        return Err("不能删除根节点".to_string());
    }

    let client = manager.client(&conn_id)?;
    let mut stack = vec![(path, false)];
    let mut deleted = 0;
    while let Some((current, children_visited)) = stack.pop() {
        if children_visited {
            client
                .delete(&current, None)
                .await
                .map_err(|error| error.to_string())?;
            deleted += 1;
            continue;
        }

        let children = client
            .list_children(&current)
            .await
            .map_err(|error| error.to_string())?;
        // 当前节点第二次出栈时，其全部子节点都已经删除。
        stack.push((current.clone(), true));
        for child in children {
            let child_path = format!("{}/{child}", current.trim_end_matches('/'));
            stack.push((child_path, false));
        }
    }
    Ok(deleted)
}

#[tauri::command]
async fn get_acl(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
) -> Result<Vec<AclEntry>, String> {
    let (acl, _) = manager
        .client(&conn_id)?
        .get_acl(&path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(acl
        .into_iter()
        .map(|entry| AclEntry {
            scheme: entry.scheme().to_string(),
            id: entry.id().to_string(),
            perms: permission_bits(entry.permission()),
        })
        .collect())
}

#[tauri::command]
async fn set_acl(
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
    acl: Vec<AclEntry>,
) -> Result<(), String> {
    if acl.is_empty() {
        return Err("ACL 列表不能为空".to_string());
    }
    let acl = acl
        .into_iter()
        .map(|entry| {
            Ok(zk::Acl::new(
                permission_from_bits(entry.perms)?,
                zk::AuthId::new(&entry.scheme, &entry.id),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    manager
        .client(&conn_id)?
        .set_acl(&path, &acl, None)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn watch_children(
    app: AppHandle,
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
) -> Result<Vec<String>, String> {
    let client = manager.client(&conn_id)?;
    let (mut children, _stat, watcher) = client
        .get_and_watch_children(&path)
        .await
        .map_err(|error| error.to_string())?;
    children.sort();

    let key = WatchKey {
        conn_id: conn_id.clone(),
        path: path.clone(),
        kind: WatchKind::Children,
    };
    let task = tauri::async_runtime::spawn(async move {
        let event = watcher.changed().await;
        let _ = app.emit(
            "zk-node-event",
            NodeEvent {
                conn_id,
                event_type: format!("{:?}", event.event_type),
                path: event.path,
                zxid: event.zxid,
            },
        );
    });
    manager.replace_watch(key, task)?;
    Ok(children)
}

#[tauri::command]
async fn watch_data(
    app: AppHandle,
    manager: State<'_, ConnectionManager>,
    conn_id: String,
    path: String,
) -> Result<NodeResult, String> {
    let client = manager.client(&conn_id)?;
    let (data, stat, watcher) = client
        .get_and_watch_data(&path)
        .await
        .map_err(|error| error.to_string())?;

    let key = WatchKey {
        conn_id: conn_id.clone(),
        path: path.clone(),
        kind: WatchKind::Data,
    };
    let task = tauri::async_runtime::spawn(async move {
        let event = watcher.changed().await;
        let _ = app.emit(
            "zk-node-event",
            NodeEvent {
                conn_id,
                event_type: format!("{:?}", event.event_type),
                path: event.path,
                zxid: event.zxid,
            },
        );
    });
    manager.replace_watch(key, task)?;
    Ok(NodeResult::from_data_and_stat(data, stat))
}

fn connections_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("connections.json"))
        .map_err(|error| error.to_string())
}

fn read_saved_connections(app: &AppHandle) -> Result<Vec<SavedConnection>, String> {
    let path = connections_file(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn write_saved_connections(app: &AppHandle, connections: &[SavedConnection]) -> Result<(), String> {
    let path = connections_file(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "无法确定配置目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let content = serde_json::to_string_pretty(connections).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_connection(app: AppHandle, connection: SavedConnection) -> Result<(), String> {
    let mut connections = read_saved_connections(&app)?;
    if let Some(existing) = connections.iter_mut().find(|item| item.id == connection.id) {
        *existing = connection;
    } else {
        connections.push(connection);
    }
    write_saved_connections(&app, &connections)
}

#[tauri::command]
fn list_saved_connections(app: AppHandle) -> Result<Vec<SavedConnection>, String> {
    read_saved_connections(&app)
}

#[tauri::command]
fn delete_saved_connection(app: AppHandle, id: String) -> Result<(), String> {
    let mut connections = read_saved_connections(&app)?;
    connections.retain(|connection| connection.id != id);
    write_saved_connections(&app, &connections)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ConnectionManager::new())
        .invoke_handler(tauri::generate_handler![
            connect,
            disconnect,
            list_children,
            get_node,
            set_data,
            create_node,
            delete_node,
            delete_node_recursive,
            get_acl,
            set_acl,
            watch_children,
            watch_data,
            save_connection,
            list_saved_connections,
            delete_saved_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
