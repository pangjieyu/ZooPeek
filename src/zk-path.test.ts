import assert from "node:assert/strict";
import test from "node:test";
import { ancestorPaths } from "./zk-path.ts";

test("ancestorPaths 从直接父节点回溯到根节点", () => {
  assert.deepEqual(ancestorPaths("/a/b/c"), ["/a/b", "/a", "/"]);
  assert.deepEqual(ancestorPaths("/a"), ["/"]);
});
