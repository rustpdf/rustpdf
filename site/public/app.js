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
