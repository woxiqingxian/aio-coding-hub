/**
 * Normalize a user-supplied Codex home path by stripping a trailing
 * `config.toml` filename (case-insensitive) and trimming whitespace.
 */
export function normalizeCustomCodexHome(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return "";

  const normalized = trimmed.replace(/[\\/]+config\.toml$/i, "");
  return normalized.trim();
}

/**
 * Build the full `config.toml` path from a `.codex` directory string.
 * Automatically picks `\` or `/` as separator based on the input style.
 */
export function buildConfigTomlPath(dir: string) {
  const trimmed = normalizeCustomCodexHome(dir);
  if (!trimmed) return "";

  const hasTrailingSeparator = /[\\/]$/.test(trimmed);
  const separator = hasTrailingSeparator ? "" : trimmed.includes("\\") ? "\\" : "/";
  return `${trimmed}${separator}config.toml`;
}
