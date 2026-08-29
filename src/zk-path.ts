/** 从被删除路径的直接父节点开始，依次返回到根节点的候选祖先。 */
export function ancestorPaths(path: string): string[] {
  if (!path || path === "/") return [];

  const ancestors: string[] = [];
  let current = path.slice(0, path.lastIndexOf("/")) || "/";
  while (true) {
    ancestors.push(current);
    if (current === "/") return ancestors;
    current = current.slice(0, current.lastIndexOf("/")) || "/";
  }
}
