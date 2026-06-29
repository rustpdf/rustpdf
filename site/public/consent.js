/* Cookie consent + gated Google Analytics.
 * GA loads ONLY after the visitor clicks "Accept". Reject = nothing loads.
 * Set GA_ID to your GA4 Measurement ID (looks like "G-XXXXXXXXXX").
 * Leave it empty and analytics simply never loads (the banner still works). */
(function () {
  var GA_ID = "G-W98TZ2NX4T"; // GA4 Measurement ID
  var KEY = "rustpdf-cookie-consent";

  function loadGA() {
    if (!GA_ID) return;
    var s = document.createElement("script");
    s.async = true;
    s.src = "https://www.googletagmanager.com/gtag/js?id=" + GA_ID;
    document.head.appendChild(s);
    window.dataLayer = window.dataLayer || [];
    function gtag() { window.dataLayer.push(arguments); }
    window.gtag = gtag;
    gtag("js", new Date());
    gtag("config", GA_ID, { anonymize_ip: true });
  }

  function hide(el) { if (el) el.parentNode.removeChild(el); }

  function showBanner() {
    var bar = document.createElement("div");
    bar.className = "cookie-banner";
    bar.setAttribute("role", "dialog");
    bar.setAttribute("aria-label", "Cookie consent");
    bar.innerHTML =
      '<p>We use only essential cookies, plus optional Google Analytics to improve the site. ' +
      'See our <a href="/legal/privacy">Privacy Policy</a>.</p>' +
      '<div class="cookie-actions">' +
      '<button type="button" class="btn btn-sm btn-ghost" data-consent="denied">Reject</button>' +
      '<button type="button" class="btn btn-sm" data-consent="granted">Accept</button>' +
      "</div>";
    bar.addEventListener("click", function (e) {
      var choice = e.target && e.target.getAttribute("data-consent");
      if (!choice) return;
      try { localStorage.setItem(KEY, choice); } catch (_) {}
      if (choice === "granted") loadGA();
      hide(bar);
    });
    document.body.appendChild(bar);
  }

  var consent = null;
  try { consent = localStorage.getItem(KEY); } catch (_) {}
  if (consent === "granted") { loadGA(); return; }
  if (consent === "denied") { return; }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", showBanner);
  } else {
    showBanner();
  }
})();
