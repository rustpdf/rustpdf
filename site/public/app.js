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

// Code tabs
document.querySelectorAll("#code-tabs .tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    const name = tab.dataset.tab;
    document.querySelectorAll("#code-tabs .tab").forEach((t) => t.classList.toggle("active", t === tab));
    document.querySelectorAll("#code-tabs .tab-pane").forEach((p) =>
      p.classList.toggle("active", p.dataset.pane === name)
    );
  });
});

// Keep the displayed prices in sync with the actual Stripe Prices (per tier).
(async () => {
  try {
    const res = await fetch("/api/pricing");
    if (!res.ok) return;
    const pricing = await res.json(); // { pro: {...}, enterprise: {...} }
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

// Buy → create a tier-specific Checkout Session → redirect to Stripe.
document.querySelectorAll(".buy-btn").forEach((btn) => {
  const tier = btn.dataset.tier;
  const err = document.querySelector(`.buy-error[data-tier="${tier}"]`);
  const original = btn.innerHTML;
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    btn.textContent = "Redirecting to checkout…";
    if (err) err.hidden = true;
    try {
      const res = await fetch("/api/checkout", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ tier }),
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
