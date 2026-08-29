//! Spike 冒烟测试：验证 zookeeper-client 的连接/列表/watcher/会话状态行为。
//! 依赖本地 docker ZK：docker run -d --name zoopeek-zk -p 2181:2181 zookeeper:3.9
//! 运行：cargo test --test zk_smoke -- --nocapture

use std::time::Duration;
use zookeeper_client as zk;

const CLUSTER: &str = "127.0.0.1:2181";

#[tokio::test(flavor = "multi_thread")]
async fn smoke_connect_list_watch() {
    let client = zk::Client::connect(CLUSTER).await.expect("connect failed");
    println!("session_id = {}", client.session_id());

    // 1. 会话状态：连接后应为 SyncConnected
    let mut state_watcher = client.state_watcher();
    let state = state_watcher.state();
    println!("session state = {:?}", state);
    assert!(matches!(
        state,
        zk::SessionState::SyncConnected | zk::SessionState::ConnectedReadOnly
    ));

    // 2. 根节点列表
    let root_children = client.list_children("/").await.expect("list / failed");
    println!("root children = {:?}", root_children);
    assert!(root_children.iter().any(|c| c == "zookeeper"));

    // 3. get_and_watch_children + 数据读取
    let (children, _stat, child_watcher) = client
        .get_and_watch_children("/zoopeek-test")
        .await
        .expect("watch children failed");
    println!("/zoopeek-test children = {:?}", children);
    assert_eq!(children.len(), 2);

    let (data, _stat) = client
        .get_data("/zoopeek-test/config")
        .await
        .expect("get_data failed");
    let data_str = String::from_utf8_lossy(&data);
    println!("/zoopeek-test/config data = {}", data_str);
    assert!(data_str.contains("3306"));

    // 4. watcher 触发验证（children）：另一个客户端建子节点，应收到 NodeChildrenChanged
    let writer = zk::Client::connect(CLUSTER).await.expect("writer connect failed");
    let create_options = zk::CreateMode::Persistent.with_acls(zk::Acls::anyone_all());
    writer
        .create("/zoopeek-test/newcomer", b"tmp", &create_options)
        .await
        .expect("create failed");

    let event = child_watcher.changed().await;
    println!("children event = {:?} path={}", event.event_type, event.path);
    assert_eq!(event.event_type, zk::EventType::NodeChildrenChanged);
    assert_eq!(event.path, "/zoopeek-test");

    // 5. watcher 触发验证（data）：改数据应收到 NodeDataChanged
    let (_data, _stat, data_watcher) = client
        .get_and_watch_data("/zoopeek-test/config")
        .await
        .expect("watch data failed");
    writer
        .set_data("/zoopeek-test/config", b"{\"v\":2}", None)
        .await
        .expect("set_data failed");

    let event = data_watcher.changed().await;
    println!("data event = {:?} path={}", event.event_type, event.path);
    assert_eq!(event.event_type, zk::EventType::NodeDataChanged);

    // 6. 清理测试节点 + 断开时应收到 Session/Closed 状态
    writer
        .delete("/zoopeek-test/newcomer", None)
        .await
        .expect("delete failed");
    drop(writer);
    drop(client);

    let final_state = tokio::time::timeout(Duration::from_secs(5), state_watcher.changed())
        .await
        .expect("timeout waiting for session close");
    println!("final session state = {:?}", final_state);
    assert_eq!(final_state, zk::SessionState::Closed);

    println!("SMOKE TEST PASSED");
}
