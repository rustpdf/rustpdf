// Mark JS active so the scroll-reveal hidden state can apply (content stays
// visible when JS is off / in headless renderers).
document.documentElement.classList.add("js");

// Reveal sections as they enter the viewport. Respects reduced-motion via CSS.
(() => {
  const items = document.querySelectorAll(".reveal");
  if (!items.length || !("IntersectionObserver" in window)) {
    items.forEach((el) => el.classList.add("in"));
    return;
  }
  const obs = new IntersectionObserver(
    (entries, o) => {
      entries.forEach((e) => {
        if (e.isIntersecting) {
          e.target.classList.add("in");
          o.unobserve(e.target);
        }
      });
    },
    { rootMargin: "0px 0px -10% 0px", threshold: 0.08 }
  );
  items.forEach((el) => obs.observe(el));
})();

// Mobile nav toggle — injected so any page with a nav gets the hamburger
// without per-page markup edits. Hidden ≥820px via CSS.
function initNavToggle() {
  const inner = document.querySelector(".nav .nav-inner");
  const links = inner && inner.querySelector(".nav-links");
  if (!inner || !links || inner.querySelector(".nav-toggle")) return;
  links.id = links.id || "nav-links";
  const btn = document.createElement("button");
  btn.className = "nav-toggle";
  btn.type = "button";
  btn.setAttribute("aria-label", "Menu");
  btn.setAttribute("aria-controls", links.id);
  btn.setAttribute("aria-expanded", "false");
  btn.innerHTML = '<span class="bars" aria-hidden="true"></span>';
  links.insertAdjacentElement("beforebegin", btn);
  const set = (open) => {
    btn.setAttribute("aria-expanded", String(open));
    links.classList.toggle("open", open);
  };
  btn.addEventListener("click", () => set(btn.getAttribute("aria-expanded") !== "true"));
  links.addEventListener("click", (e) => { if (e.target.closest("a")) set(false); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") set(false); });
  document.addEventListener("click", (e) => {
    if (links.classList.contains("open") && !inner.contains(e.target)) set(false);
  });
}
initNavToggle();

// Code / install tabs: accessible tablist (role + aria-selected) with roving
// focus and arrow-key navigation. Works for every .tabs block on the page.
document.querySelectorAll(".tabs").forEach((tabs, ti) => {
  const bar = tabs.querySelector(".tab-bar");
  const tabBtns = [...tabs.querySelectorAll(".tab")];
  const panes = [...tabs.querySelectorAll(".tab-pane")];
  if (!bar || !tabBtns.length) return;
  bar.setAttribute("role", "tablist");
  const paneByName = new Map(panes.map((p) => [p.dataset.pane, p]));
  const select = (btn, focus) => {
    tabBtns.forEach((b) => {
      const on = b === btn;
      b.classList.toggle("active", on);
      b.setAttribute("aria-selected", String(on));
      b.setAttribute("tabindex", on ? "0" : "-1");
    });
    panes.forEach((p) => p.classList.toggle("active", p.dataset.pane === btn.dataset.tab));
    if (focus) btn.focus();
  };
  tabBtns.forEach((btn, i) => {
    const name = btn.dataset.tab;
    const pane = paneByName.get(name);
    btn.id = btn.id || `tab-${ti}-${name}`;
    btn.setAttribute("role", "tab");
    if (pane) {
      pane.id = pane.id || `panel-${ti}-${name}`;
      pane.setAttribute("role", "tabpanel");
      pane.setAttribute("aria-labelledby", btn.id);
      pane.setAttribute("tabindex", "0");
      btn.setAttribute("aria-controls", pane.id);
    }
    btn.addEventListener("click", () => select(btn));
    btn.addEventListener("keydown", (e) => {
      let n = null;
      if (e.key === "ArrowRight" || e.key === "ArrowDown") n = (i + 1) % tabBtns.length;
      else if (e.key === "ArrowLeft" || e.key === "ArrowUp") n = (i - 1 + tabBtns.length) % tabBtns.length;
      else if (e.key === "Home") n = 0;
      else if (e.key === "End") n = tabBtns.length - 1;
      if (n !== null) { e.preventDefault(); select(tabBtns[n], true); }
    });
  });
  // Ensure exactly one selected tab (default to the first).
  select(tabBtns.find((b) => b.classList.contains("active")) || tabBtns[0]);
});

// Keep the displayed prices in sync with the actual Stripe Prices (per tier).
// Only runs on pages that actually show pricing (skips docs/legal/feature pages).
(async () => {
  if (!document.querySelector("[id^='price-amount-'], .buy-btn")) return;
  try {
    const res = await fetch("/api/pricing");
    if (!res.ok) return;
    const pricing = await res.json(); // { pro: {...}, enterprise: {...} }
    window.__rustpdfPricing = pricing; // reused by the begin_checkout event
    for (const tier of Object.keys(pricing)) {
      const p = pricing[tier];
      if (!p) continue;
      const amount = document.getElementById(`price-amount-${tier}`);
      const interval = document.getElementById(`price-interval-${tier}`);
      const buyPrice = document.querySelector(`.buy-price[data-tier="${tier}"]`);
      if (amount) amount.textContent = p.formatted;
      if (interval) interval.textContent = p.intervalLabel;
      if (buyPrice) buyPrice.textContent = `${p.formatted}/${(p.interval || "yr").slice(0, 2)}`;
    }
  } catch {
    /* keep the static fallback prices */
  }
})();

// Free-trial forms: email → server mints a short-lived token and emails it. The
// token is never returned to the page (lead quality + anti-farming), so success
// just tells the user to check their inbox. Class-based + per-form so the same
// widget works on the home #verify block and on every language docs page.
document.querySelectorAll(".trial-form").forEach((form) => {
  const email = form.querySelector('input[type="email"]');
  const honeypot = form.querySelector('input[name="website"]');
  const consent = form.querySelector('input[name="consent"]');
  const btn = form.querySelector('button[type="submit"]');
  const msg = form.querySelector(".trial-msg");
  if (!email || !btn || !msg) return;
  const original = btn.textContent;

  const show = (text, ok) => {
    msg.textContent = text;
    msg.classList.toggle("ok", !!ok);
    msg.classList.toggle("err", !ok);
    msg.hidden = false;
  };

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (honeypot && honeypot.value.trim()) return; // bot
    const addr = (email.value || "").trim();
    if (!addr || !email.checkValidity()) {
      show("Please enter a valid email address.", false);
      email.focus();
      return;
    }
    btn.disabled = true;
    btn.textContent = "Sending…";
    msg.hidden = true;
    try {
      const res = await fetch("/api/trial", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          email: addr,
          consent: !!(consent && consent.checked),
          website: honeypot ? honeypot.value : "",
        }),
      });
      if (res.status === 429) {
        show("Too many requests from your network. Please try again later.", false);
        return;
      }
      if (!res.ok) throw new Error("trial_failed");
      show("Check your inbox — your free 5-day trial token is on its way.", true);
      form.reset();
      if (window.gtag) {
        window.gtag("event", "generate_lead", { currency: "USD", value: 0 });
      }
    } catch (_e) {
      show("Could not send the trial token. Please try again or email sales@casefy.io.", false);
    } finally {
      btn.disabled = false;
      btn.textContent = original;
    }
  });
});

// Reads the Google Ads click id captured by consent.js — sent at checkout so the
// server can attribute the sale back to the ad click (offline conversion).
function readGclid() {
  var m = document.cookie.match(/(?:^|;\s*)gclid=([^;]+)/);
  return m ? decodeURIComponent(m[1]) : "";
}

// Buy → create a tier-specific Checkout Session → redirect to Stripe.
document.querySelectorAll(".buy-btn").forEach((btn) => {
  const tier = btn.dataset.tier;
  const err = document.querySelector(`.buy-error[data-tier="${tier}"]`);
  const original = btn.innerHTML;
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    btn.textContent = "Redirecting to checkout…";
    if (err) err.hidden = true;
    // Conversion signal: user started checkout for a paid tier.
    // gtag only exists once the visitor accepted analytics cookies (consent.js).
    const price = (window.__rustpdfPricing || {})[tier] || {};
    if (window.gtag) {
      window.gtag("event", "begin_checkout", {
        currency: price.currency || "USD",
        value: price.amount || undefined,
        items: [{ item_id: tier, item_name: "rust-pdf " + tier }],
      });
    }
    try {
      const res = await fetch("/api/checkout", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ tier, gclid: readGclid() }),
      });
      if (!res.ok) throw new Error("checkout_failed");
      const { url } = await res.json();
      if (!url) throw new Error("no_url");
      window.location.href = url;
    } catch (e) {
      btn.disabled = false;
      btn.innerHTML = original;
      if (err) {
        err.textContent = "Could not start checkout. Please try again or email sales@casefy.io.";
        err.hidden = false;
      }
    }
  });
});
