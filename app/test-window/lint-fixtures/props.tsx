// Lint fixture: classes reach the page through class=, a prop ending in ClassName, a spread object's
// className and an unreadable spread; each is reported.
function Box(_props: { class?: string; innerClassName?: string; className?: string }) {
  return null;
}

export function Props({ rest }: { rest: Record<string, string> }) {
  return (
    <div>
      <Box class="p-[7px]" />
      <Box innerClassName="text-sm" />
      <Box {...{ className: 'bg-red-500' }} />
      <Box {...rest} />
    </div>
  );
}
