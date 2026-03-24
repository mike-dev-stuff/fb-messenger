console.log("[Messenger App] Custom JS loaded ✓");

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
