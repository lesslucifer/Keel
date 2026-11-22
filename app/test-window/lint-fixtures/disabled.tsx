// Lint fixture: three ways to turn a rule off inline. Keel's rule has no escape hatch, so each
// class below is still reported.
/* eslint-disable */
export function Disabled() {
  return (
    <p className="dark:bg-ink">
      {/* eslint-disable-next-line design/vocabulary */}
      <span className="p-[13px]">a</span>
      <span className="text-sm">b</span> {/* eslint-disable-line */}
    </p>
  );
}
