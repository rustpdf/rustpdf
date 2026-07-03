/* Cookie consent + Google Analytics with Consent Mode v2.
 *
 * Unlike a hard on/off gate, the gtag ALWAYS loads. Consent Mode governs what
 * it may store/send: denied by default in the EU/EEA/UK/CH (GDPR — needs opt-in
 * via the banner), granted by default elsewhere (with the banner offering
 * opt-out). Denied traffic still sends cookieless pings the model can use.
 *
 * Set GA_ID to your GA4 Measurement ID ("G-XXXXXXXXXX"). Empty = nothing loads. */
(function () {
  var GA_ID = "G-W98TZ2NX4T"; // GA4 Measurement ID
  var KEY = "rustpdf-cookie-consent";

  // Regions where analytics/ads storage must default to DENIED (opt-in).
  var EU = [
    "AT","BE","BG","HR","CY","CZ","DK","EE","FI","FR","DE","GR","HU","IE","IT",
    "LV","LT","LU","MT","NL","PL","PT","RO","SK","SI","ES","SE", // EU-27
    "IS","LI","NO", // EEA
    "GB","CH", // UK + Switzerland
  ];

  // --- Capture the Google Ads click id for offline conversion attribution. ---
  // First-party, functional cookie (90d) — read at checkout, uploaded server-side
  // when the sale completes. Also handled by Consent Mode's url_passthrough.
  try {
    var gm = location.search.match(/[?&]gclid=([^&]+)/);
    if (gm) {
      var exp = new Date(Date.now() + 90 * 864e5).toUTCString();
      document.cookie =
        "gclid=" + encodeURIComponent(decodeURIComponent(gm[1])) +
        ";path=/;expires=" + exp + ";SameSite=Lax";
    }
  } catch (_) {}

  // --- gtag bootstrap (available immediately, before the script loads). ---
  window.dataLayer = window.dataLayer || [];
  function gtag() { window.dataLayer.push(arguments); }
  window.gtag = gtag;

  // Consent Mode v2 defaults — MUST run before any config/event.
  gtag("consent", "default", {
    ad_storage: "denied",
    analytics_storage: "denied",
    ad_user_data: "denied",
    ad_personalization: "denied",
    region: EU,
    wait_for_update: 500,
  });
  gtag("consent", "default", {
    // Everywhere not listed above (US, BR, IN, …): granted, banner offers opt-out.
    ad_storage: "granted",
    analytics_storage: "granted",
    ad_user_data: "granted",
    ad_personalization: "granted",
  });
  // Recover conversions without cookies by passing ad ids through URLs.
  gtag("set", "url_passthrough", true);
  gtag("set", "ads_data_redaction", true);

  function updateConsent(state) {
    gtag("consent", "update", {
      ad_storage: state,
      analytics_storage: state,
      ad_user_data: state,
      ad_personalization: state,
    });
  }

  // Apply the visitor's saved choice (overrides the regional default).
  var saved = null;
  try { saved = localStorage.getItem(KEY); } catch (_) {}
  if (saved === "granted") updateConsent("granted");
  else if (saved === "denied") updateConsent("denied");

  // Always load gtag — Consent Mode decides what it may send.
  if (GA_ID) {
    var s = document.createElement("script");
    s.async = true;
    s.src = "https://www.googletagmanager.com/gtag/js?id=" + GA_ID;
    document.head.appendChild(s);
    gtag("js", new Date());
    gtag("config", GA_ID, { anonymize_ip: true });
  }

  // Banner only when the visitor hasn't chosen yet.
  if (saved === "granted" || saved === "denied") return;

  function hide(el) { if (el && el.parentNode) el.parentNode.removeChild(el); }

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
      updateConsent(choice);
      hide(bar);
    });
    document.body.appendChild(bar);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", showBanner);
  } else {
    showBanner();
  }
})();
