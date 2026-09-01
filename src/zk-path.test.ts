import assert from "node:assert/strict";
import test from "node:test";
import {
  ancestorPaths,
  isPathInScope,
  pathChain,
  validateZkPath,
} from "./zk-path.ts";

test("ancestorPaths 从直接父节点回溯到根节点", () => {
  assert.deepEqual(ancestorPaths("/a/b/c"), ["/a/b", "/a", "/"]);
  assert.deepEqual(ancestorPaths("/a"), ["/"]);
});

test("validateZkPath 只接受规范绝对路径", () => {
  for (const path of ["/", "/a", "/中文/节点"]) {
    assert.equal(validateZkPath(path), true, path);
  }
  for (const path of ["", "a", "/a/", "/a//b", "/./a", "/a/../b"]) {
    assert.equal(validateZkPath(path), false, path);
  }
});

test("isPathInScope 按路径段判断范围", () => {
  assert.equal(isPathInScope("/foo", "/foo"), true);
  assert.equal(isPathInScope("/foo/bar", "/foo"), true);
  assert.equal(isPathInScope("/foobar", "/foo"), false);
  assert.equal(isPathInScope("/anything", "/"), true);
});

test("pathChain 返回从根到目标的逐层路径", () => {
  assert.deepEqual(pathChain("/a/b/c"), ["/", "/a", "/a/b", "/a/b/c"]);
  assert.deepEqual(pathChain("/"), ["/"]);
});
