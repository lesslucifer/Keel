// Lint fixture: the other ways a class or a raw value reaches the page - an inline style, and the
// DOM written by hand - each reported.
export function Siblings() {
  return <p style={{ color: 'red' }}>a</p>;
}

export function byHand(el: HTMLElement) {
  el.className = 'text-sm';
  el.classList.add('p-[3px]');
  el.setAttribute('class', 'bg-red-500');
  el.setAttribute('style', 'color: red');
}
