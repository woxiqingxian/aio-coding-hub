type SourceLike = {
  source_git_url?: string | null;
  source_branch?: string | null;
  source_subdir?: string | null;
};

export function sourceKey(
  skill: Required<Pick<SourceLike, "source_git_url" | "source_branch" | "source_subdir">>
) {
  return `${skill.source_git_url}#${skill.source_branch}:${skill.source_subdir}`;
}

export function repoKey(skill: Required<Pick<SourceLike, "source_git_url" | "source_branch">>) {
  return `${skill.source_git_url}#${skill.source_branch}`;
}

export function normalizeRepoPath(input: string) {
  const raw = input.trim();
  if (!raw || raw.startsWith("local://")) return "";

  if (raw.startsWith("git@")) {
    const withoutPrefix = raw.slice("git@".length);
    const [host, path = ""] = withoutPrefix.split(":");
    const normalizedPath = path.replace(/\.git$/i, "").replace(/^\/+|\/+$/g, "");
    return normalizedPath ? normalizedPath : host.replace(/\.git$/i, "");
  }

  try {
    const url = new URL(raw);
    return url.pathname.replace(/\.git$/i, "").replace(/^\/+|\/+$/g, "");
  } catch {
    return raw
      .replace(/^https?:\/\//i, "")
      .replace(/^[^/]+\/+/, "")
      .replace(/\.git$/i, "")
      .replace(/^\/+|\/+$/g, "");
  }
}

export function repoPrefixFromGitUrl(input: string) {
  const repoPath = normalizeRepoPath(input);
  if (!repoPath) return null;
  const segments = repoPath.split("/").filter(Boolean);
  if (segments.length >= 2) {
    return segments.slice(-2).join("/");
  }
  return segments.length > 0 ? segments[segments.length - 1] : null;
}

export function repositoryWebUrl(input: string) {
  const raw = input.trim();
  if (!raw || raw.startsWith("local://")) return null;

  if (raw.startsWith("git@")) {
    const withoutPrefix = raw.slice("git@".length);
    const [host, path = ""] = withoutPrefix.split(":");
    const normalizedPath = path.replace(/\.git$/i, "").replace(/^\/+/, "");
    if (!host || !normalizedPath) return null;
    return `https://${host}/${normalizedPath}`;
  }

  try {
    const url = new URL(raw);
    url.hash = "";
    url.search = "";
    url.pathname = url.pathname.replace(/\.git$/i, "");
    return url.toString().replace(/\/+$/, "");
  } catch {
    return null;
  }
}

export function sourceHint(skill: SourceLike) {
  const sourceGitUrl = skill.source_git_url?.trim();
  const sourceBranch = skill.source_branch?.trim();
  const sourceSubdir = skill.source_subdir?.trim();
  if (!sourceGitUrl || !sourceBranch || !sourceSubdir) return "";
  return `${sourceGitUrl}#${sourceBranch}:${sourceSubdir}`;
}

export function displaySkillName(name: string, sourceGitUrl?: string | null) {
  const prefix = sourceGitUrl ? repoPrefixFromGitUrl(sourceGitUrl) : null;
  return prefix ? `${prefix}/${name}` : name;
}
