// A path, shown whole in mono as text, wrapping anywhere rather than cut.
export function Path({ path }: { path: string }) {
  return <code className="break-all font-mono text-caption text-ink">{path}</code>;
}
