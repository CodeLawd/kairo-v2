// Port of src/preload/audience-slido.ts (prompt 047). The page world has no
// isolated preload here, so everything this posts is untrusted on the Rust side.
// `kairoSlido` ({ code, moderator }) and `qaSelectors` (selectors.js, built from
// slido-selectors.ts unchanged) are defined above this file in the same scope.
const post = window.ipc && window.ipc.postMessage ? window.ipc.postMessage.bind(window.ipc) : null;
const send = value => { if (post) post(JSON.stringify(value)); };
const { code, moderator } = kairoSlido;
const { SLIDO_SELECTORS, readSlidoPage, pendingQueueButton } = qaSelectors;
window.addEventListener('DOMContentLoaded', () => {
  let joined = false;
  let pendingOpened = false;
  let lastHref = '';
  const read = () => {
    // Wry has no did-navigate-in-page event: report location changes here.
    if (location.href !== lastHref) { lastHref = location.href; send({ kairoNavigated: true }); }
    const participant = location.hostname === 'app.sli.do';
    if (participant && !location.pathname.startsWith('/event/') && !joined && /^[a-zA-Z0-9-]{1,32}$/.test(code)) {
      const input = document.querySelector(SLIDO_SELECTORS.joinInput);
      const button = document.querySelector(SLIDO_SELECTORS.joinButton);
      if (input && button && !button.disabled && input.value !== code) {
        // Only submit the event-code search. Never address ask, vote or moderation buttons.
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
        setter?.call(input, code);
        input.dispatchEvent(new Event('input', { bubbles: true }));
        input.dispatchEvent(new Event('change', { bubbles: true }));
      } else if (input?.value === code && button && !button.disabled) {
        button.click();
        joined = true;
      }
    }
    // The join page reports a wrong code; the event page reports its name.
    if (participant) {
      const page = readSlidoPage(document, location, 'public');
      if (!moderator || page.notFound || page.eventName) send(moderator ? { ...page, questions: [], empty: false } : page);
      if (!moderator) return;
    }
    if (location.hostname !== 'admin.sli.do') return;
    // Send a heartbeat even when content is unchanged, so selector failure after
    // a successful parse is detected too. Rust deduplicates the queue updates.
    send(readSlidoPage(document, location, 'moderator'));
    // Read the current live cards before opening the pending view.
    if (moderator && !pendingOpened) {
      const pending = pendingQueueButton(document);
      if (pending) { pending.click(); pendingOpened = true; }
    }
  };
  read();
  const timer = setInterval(read, 1000);
  window.addEventListener('unload', () => clearInterval(timer), { once: true });
});
