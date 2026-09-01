import assert from "node:assert/strict";
import test from "node:test";
import {
  describeMatchHighlight,
  isValidSearchQuery,
} from "./zk-search.ts";

test("isValidSearchQuery 拒绝空查询与非法路径", () => {
  assert.equal(isValidSearchQuery(""), false);
  assert.equal(isValidSearchQuery("   "), false);
  assert.equal(isValidSearchQuery("/a/"), false);
  assert.equal(isValidSearchQuery("/a//b"), false);
  assert.equal(isValidSearchQuery("/./a"), false);
  assert.equal(isValidSearchQuery("/a/../b"), false);
});

test("isValidSearchQuery 接受简单名与合法路径", () => {
  assert.equal(isValidSearchQuery("order"), true);
  assert.equal(isValidSearchQuery("  order  "), true);
  assert.equal(isValidSearchQuery("/services/order-api"), true);
  assert.equal(isValidSearchQuery("/中文/节点"), true);
});

test("describeMatchHighlight 区分匹配类型", () => {
  assert.equal(
    describeMatchHighlight({
      path: "/a",
      name: "a",
      score: 0,
      match_target: "path",
      highlight_ranges: [],
    }),
    "路径匹配"
  );
  assert.equal(
    describeMatchHighlight({
      path: "/a",
      name: "order-api",
      score: 0,
      match_target: "name",
      highlight_ranges: [],
    }),
    "子序列匹配"
  );
  assert.equal(
    describeMatchHighlight({
      path: "/a",
      name: "order",
      score: 0,
      match_target: "name",
      highlight_ranges: [[0, 5]],
    }),
    "完全匹配"
  );
  assert.equal(
    describeMatchHighlight({
      path: "/a",
      name: "xorder-api",
      score: 0,
      match_target: "name",
      highlight_ranges: [[1, 6]],
    }),
    "包含匹配"
  );
  assert.equal(
    describeMatchHighlight({
      path: "/a",
      name: "order-api",
      score: 0,
      match_target: "name",
      highlight_ranges: [[0, 2]],
    }),
    "前缀匹配"
  );
});