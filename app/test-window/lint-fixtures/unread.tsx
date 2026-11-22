// Lint fixture: values the rule cannot read reaching a class (an unknown call, an import, a
// parameter) are each reported; a reassigned binding is read whole.
import { SCENARIO_NAMES } from '../../src/window/fake/fixtures.ts';

declare function tw(s: string): string;

function Box(props: { className?: string }) {
  return <i className={props.className} />;
}

let m = 'flex';
m = 'bg-red-500';

export function Unread() {
  return (
    <div className={tw('p-[3px]')}>
      <span className={SCENARIO_NAMES[0]}>a</span>
      <span className={m}>b</span>
      <Box className="p-xs" />
    </div>
  );
}
