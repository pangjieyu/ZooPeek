export function validateZkPath(path: string): boolean {
  if (path === "/") return true;
  if (!path.startsWith("/") || path.endsWith("/")) return false;
  const segments = path.slice(1).split("/");
  return segments.every(
    (segment) => segment.length > 0 && segment !== "." && segment !== ".."
  );
}

export function isPathInScope(path: string, scopePath: string): boolean {
  if (!validateZkPath(path) || !validateZkPath(scopePath)) return false;
  return scopePath === "/" || path === scopePath || path.startsWith(`${scopePath}/`);
}

export function pathChain(path: string): string[] {
  if (!validateZkPath(path)) return [];
  if (path === "/") return ["/"];
  const chain = ["/"];
  let current = "";
  for (const segment of path.slice(1).split("/")) {
    current += `/${segment}`;
    chain.push(current);
  }
  return chain;
}

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
