# 协作与发布规范

## 分支模型

| 分支 | 角色 | 规则 |
|---|---|---|
| `develop` | 默认分支，日常开发 | 直接提交或 PR 合入均可 |
| `main` | 发布分支 | 只接受 develop 的 squash PR；禁 force push / 删除；要求线性历史 |

## 发版流程

1. 确认版本号三处一致：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`
2. 发起 PR：`develop` → `main`，**必须 squash 合并**（main 线性历史要求，merge commit 会被拒）
3. 在 main 头部打 tag：`git tag -a vX.Y.Z <main-sha> -m "..."` 并推送
4. 推送 `v*` tag 自动触发 Release 流水线，产出 Windows/macOS/Linux 安装包并创建 **draft release**
5. 人工检查 draft 的附件与说明后点 **Publish release**

## 发版后必须同步 develop

squash 合并会在 main 产生新提交，导致 develop 与 main 历史分叉，下一次发版 PR 必冲突。因此每次发版后：

```bash
git checkout develop
git reset --hard origin/main   # 或 git rebase origin/main，有未发版改动时用 rebase
git push --force-with-lease origin develop
```

## 重打 tag

tag 未发布 release 且没有外部用户时允许移动：

```bash
git push origin :refs/tags/vX.Y.Z   # 删远端
git tag -d vX.Y.Z                   # 删本地
git tag -a vX.Y.Z <new-sha> -m "..."
git push origin vX.Y.Z
```

tag 已关联正式 release 后禁止移动，用新版本号。

## 环境约束备忘

- GitHub Actions 统一使用 Node 24 版本的 Action，全部固定到完整 commit SHA
- 本机全局 gitignore 的 `Icon?` 规则会吞掉 `icons/` 目录，仓库 `.gitignore` 已用 `!src-tauri/icons/` 显式豁免，新增平台图标目录时检查 `git check-ignore`
