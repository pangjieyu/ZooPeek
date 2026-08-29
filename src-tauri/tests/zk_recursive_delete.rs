//! 递归删除链路测试：与 src/lib.rs 的 delete_node_recursive 命令使用相同的
//! 迭代栈算法，直接对 docker ZK（127.0.0.1:2181）验证。
//! 运行：cargo test --test zk_recursive_delete -- --nocapture

use zookeeper_client as zk;

const CLUSTER: &str = "127.0.0.1:2181";

async fn delete_recursive(client: &zk::Client, path: &str) -> u32 {
    let mut stack = vec![(path.to_string(), false)];
    let mut deleted = 0;
    while let Some((current, children_visited)) = stack.pop() {
        if children_visited {
            client.delete(&current, None).await.expect("delete failed");
            deleted += 1;
            continue;
        }
        let children = client.list_children(&current).await.expect("list failed");
        stack.push((current.clone(), true));
        for child in children {
            stack.push((format!("{}/{child}", current.trim_end_matches('/')), false));
        }
    }
    deleted
}

#[tokio::test(flavor = "multi_thread")]
async fn recursive_delete_deep_tree() {
    let client = zk::Client::connect(CLUSTER).await.expect("connect failed");
    let options = zk::CreateMode::Persistent.with_acls(zk::Acls::anyone_all());

    // 构造三层树：/del-test/a/b/c、d，/del-test/e
    for path in [
        "/del-test",
        "/del-test/a",
        "/del-test/a/b",
        "/del-test/a/b/c",
        "/del-test/a/b/d",
        "/del-test/e",
    ] {
        client
            .create(path, b"x", &options)
            .await
            .expect("create failed");
    }

    let deleted = delete_recursive(&client, "/del-test").await;
    println!("deleted {} nodes", deleted);
    assert_eq!(deleted, 6);

    // 删除后节点应不存在
    let result = client.get_data("/del-test/a/b/c").await;
    assert!(result.is_err(), "节点应已被删除");
    println!("RECURSIVE DELETE TEST PASSED");
}
