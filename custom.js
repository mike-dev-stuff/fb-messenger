console.log("[Messenger App] Custom JS loaded ✓");

// Watch for unread message count in the page title (e.g. "Messenger (3)")
(function observeUnreadCount() {
  const titleEl = document.querySelector("title");
  if (!titleEl) return;

  function parseAndSendCount() {
    const match = document.title.match(/\((\d+)\)/);
    const count = match ? parseInt(match[1], 10) : 0;
    if (window.messengerApp && window.messengerApp.setBadgeCount) {
      window.messengerApp.setBadgeCount(count);
    }
  }

  // Check immediately
  parseAndSendCount();

  // Watch for title changes
  const observer = new MutationObserver(parseAndSendCount);
  observer.observe(titleEl, { childList: true, characterData: true, subtree: true });
})();

// Intercept clicks on links that navigate away from Messenger
document.addEventListener("click", (e) => {
  const anchor = e.target.closest("a[href]");
  if (!anchor) return;

  const href = anchor.href;
  if (!href) return;

  // Allow normal Messenger navigation
  if (href.startsWith("https://www.facebook.com/messages")) return;

  // Open everything else in the default browser
  e.preventDefault();
  e.stopPropagation();
  if (window.messengerApp && window.messengerApp.openExternal) {
    window.messengerApp.openExternal(href);
  }
}, true);
